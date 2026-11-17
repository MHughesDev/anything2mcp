# `a2m-pack-web-browser`

The base of the web pack: a browser engine that speaks the DevTools protocol to Chrome, Edge, or Chromium, launched with a data directory the engine owns. It launches and recycles browsers, opens tabs, locates elements by role and name, acts on them, waits on events, observes the network, and reports crashes. It is not a driver and registers nothing; `a2m-pack-web-ui` and `a2m-pack-web-probe` are built on it.

## Facts

| | |
| --- | --- |
| Side | Pack: web |
| Layer | 9, Pack bases |
| May depend on | Layers 1 to 5 |
| Depends on | `a2m-core` for `Failure`, `Redact`, `Digest`, `Availability`, and the names of a profile key; `a2m-provider` for the pack context: the scoped supervisor, checked paths, the redactor, and `Deadline`. Outside: `tokio`, `serde`, and `serde_json`, and nothing else. The base is a DevTools-protocol client of the crate's own ([decision 0070](../../decisions/0070-the-browser-engine-stands-on-its-own-client-over-the-debugging-pipe.md)); there is no outside base library |
| Built in | Slice 0.4 chose the base. Slice 1.1 built the client, launch, tabs, locating, reading, waiting, snapshots, input, the observing tab, and in-page requests. Package 2.3 the visible window. Package 8.5 the hard cases |
| Carries out | Spec sections [302](../../spec/3-packs/302-the-web-pack-browser-engine.md), [215](../../spec/2-kernel/215-sessions-and-the-session-keeper.md) (profiles and tabs), [204](../../spec/2-kernel/204-supervising-subsystems.md) (for the browser), [225](../../spec/2-kernel/225-performance.md) (the browser budgets); decisions [0004](../../decisions/0004-rust-browser-engine-on-the-devtools-protocol.md), [0070](../../decisions/0070-the-browser-engine-stands-on-its-own-client-over-the-debugging-pipe.md), [0031](../../decisions/0031-the-browser-always-runs-with-a-profile-the-engine-owns.md), [0032](../../decisions/0032-one-live-session-per-identity-shared-by-tabs-with-a-single-renewer.md), [0110](../../decisions/0110-the-browser-engines-interface-as-building-it-settled-it.md), [0111](../../decisions/0111-an-observing-tabs-lock-as-built-and-what-the-browser-was-measured-to-do.md) |

## Owns

- Launching a browser program, through the supervisor the pack context scopes to the programs its manifest declares, with an engine-owned data directory, one per target, environment, and identity, given as a checked path.
- One browser process per profile, shared by many tabs.
- Tabs with a fixed environment: viewport, locale, time zone, color scheme, reduced motion.
- Resolving a query to exactly one rendered element, through a helper injected into an isolated world, across open shadow roots.
- Actionability checks and input, with the check and the focus in one in-page call.
- Waits that end on a protocol event or an in-page observer, with a deadline. No polling.
- Building a snapshot from the accessibility tree, in the form of [steps, snapshots, and diffs](../../reference/steps-snapshots-and-diffs.md#snapshots), so that a run's evidence and discovery's findings use one builder.
- Requests sent from inside the page.
- The posture of each tab: one that may act, or one that may only observe (below).
- Detecting a crash, a disconnect, and a dialog, and saying for each act whether its input reached the page.

Designed and not built: recycling by run count, age, and memory; declared frames; platform profiles as data; the stream of network exchanges; pop-ups and downloads.

It must never: start a process except through its scoped supervisor; connect to any browser process it did not start; resolve a path itself, or open a data directory it was not handed as a checked path; read or copy another program's cookie store; hide that it is automation, alter its fingerprint, or answer a challenge; know about screen models, widget drivers, endpoints, or modes; decide whether a failure means a write may have landed; retry.

## Interface

As built. The base is behind this interface and does not appear in it: no type of the protocol client is public, so that a later decision can change the base without touching the crates above (hypotheses H-01 to H-03).

```rust
pub const PROFILE_ROOT: &str = "profiles";   // the root the pack's manifest declares under write_roots
pub const BROWSER_KIND: &str = "browser";    // the subsystem kind the supervisor counts
pub const KNOWN_PLACES: &[KnownPlace];        // { os, path }: where a browser is installed, as data
pub fn known_places() -> impl Iterator<Item = &'static str>;   // those of this operating system
pub const FIXED_ARGUMENTS: &[&str];           // the launch arguments, before the data directory
pub use Deadline;                             // the provider contract's, re-exported

#[non_exhaustive]
pub struct BrowserSettings {
    pub program: Option<PathBuf>,         // a program a person named in settings
    pub known_digests: Vec<Digest>,       // not empty: a program with another digest is closed
    pub launch_deadline: Duration,        // 120 s: a ceiling for a cold machine, not a budget
    pub close_deadline: Duration,         // 10 s
    pub call_deadline: Duration,          // 30 s: opening a tab, answering an intercepted request
    pub max_message_bytes: usize,         // 64 MiB: a larger message closes the link
    pub max_read_chars: usize,            // 1 Mi: a longer read is `TooLarge`, never cut
}
impl Default for BrowserSettings;
impl BrowserSettings { pub fn with_program(self, PathBuf) -> Self; pub fn with_known_digests(self, Vec<Digest>) -> Self; }

/// One per engine process. Cheap to clone; every clone is the same set.
pub struct Browsers;
impl Browsers {
    /// Takes the scoped supervisor, the profile root, and the redactor from the pack's context.
    pub fn new(ctx: &PackContext, settings: BrowserSettings) -> Result<Self, Failure>;
    pub fn availability(&self) -> Availability;          // Ready or ComponentMissing; starts nothing
    pub async fn open(&self, profile: &ProfileKey, show: Visibility) -> Result<Browser, BrowserError>;
    pub fn observer(&self) -> Observer;
    pub fn list(&self) -> Vec<OpenBrowser>;               // for doctor.run and the supervisor listing
    pub async fn close(&self, profile: &ProfileKey) -> Cleanup;   // the supervisor's report
    pub async fn shutdown(&self) -> Cleanup;
}

pub struct ProfileKey { pub target: TargetName, pub environment: Environment, pub identity: Identity }
impl ProfileKey { pub fn place(&self) -> String; }        // the data directory, relative to the root
#[non_exhaustive] pub enum Visibility { Background }
pub enum BrowserHealth { Idle, Busy, Exited }
pub struct OpenBrowser { pub profile, pub subsystem: SubsystemId, pub product: String,
                         pub program_digest: Digest, pub verified: bool, pub tabs: usize,
                         pub runs: u64, pub health: BrowserHealth }

pub struct Browser;                                        // cheap to clone; one process
impl Browser {
    pub async fn acting_tab(&self, env: &PageEnvironment) -> Result<ActTab, BrowserError>;
    pub fn health(&self) -> BrowserHealth;
    pub fn product(&self) -> &str;                         // what the browser reports itself to be
    pub fn subsystem(&self) -> SubsystemId;
}

pub struct Observer;                                       // a `Browsers` that can only look
impl Observer {
    pub async fn observing_tab(&self, profile: &ProfileKey, env: &PageEnvironment, allow: &SafeRequests)
        -> Result<ObserveTab, BrowserError>;
    pub fn availability(&self) -> Availability;
}

pub struct PageEnvironment { pub viewport: (u32, u32), pub locale: String, pub time_zone: String,
                             pub dark: bool, pub reduced_motion: bool }    // has a Default
pub struct Address;                                        // an absolute http or https address
impl Address { pub fn new(text: &str) -> Result<Self, Failure>; pub fn as_str(&self) -> &str; }

/// Everything that only looks. Both tab kinds have it. Each method returns a future that is Send.
pub trait Look {
    async fn goto(&self, address: &Address, deadline: Deadline) -> Result<Loaded, BrowserError>;
    async fn locate(&self, query: &Query, deadline: Deadline) -> Result<Element, BrowserError>;
    async fn read(&self, element: &Element, what: &ReadWhat, deadline: Deadline) -> Result<RawValue, BrowserError>;
    async fn wait(&self, until: &Primitive, deadline: Deadline) -> Result<(), BrowserError>;
    async fn snapshot(&self, limits: &SnapshotLimits, deadline: Deadline) -> Result<Snapshot, BrowserError>;
}
pub struct ActTab;     impl Look for ActTab;       // closed when dropped
pub struct ObserveTab; impl Look for ObserveTab;   // closed when dropped
impl ActTab {
    pub async fn input(&self, element: &Element, how: &Input, deadline: Deadline) -> Result<(), BrowserError>;
    pub async fn fetch(&self, request: &PageRequest, deadline: Deadline) -> Result<PageResponse, BrowserError>;
    pub async fn answer_dialog(&self, how: &DialogAnswer, deadline: Deadline) -> Result<(), BrowserError>;
}
impl ObserveTab { pub fn blocked(&self) -> Vec<RequestShape>; }   // what it refused, taken out

pub struct Loaded { pub address: String, pub title: String, pub status: Option<u16> }
pub struct Element;                                        // one element of one page generation and one tab
pub enum By { RoleName { role, name }, Label { text }, Placeholder { text }, Text { text, contains },
              TestId { id }, Title { text }, Css { selector } }
pub struct Query { pub by: By, pub within: Option<Box<Query>> }
impl Query { pub fn new(by: By) -> Self; pub fn role(role: &str, name: &str) -> Self; pub fn within(self, scope: Query) -> Self; }
pub struct TextMatch { pub text: String, pub contains: bool }
pub enum Primitive { Visible(Query), Hidden(Query), Enabled(Query), Text(Query, TextMatch),
                     Url(TextMatch), All(Vec<Primitive>), Any(Vec<Primitive>) }
pub enum ReadWhat { Value, Text, Checked, SelectedText, Attribute { name: String } }
pub enum RawValue { Text(String), Flag(bool), Absent }
pub enum Input { Fill(String), Type(String), NativeSet(String), Press(Key), Click, Check(bool), SelectByValue(String) }
pub enum Key { Enter, Tab, Escape, Backspace, Delete, ArrowUp, ArrowDown, ArrowLeft, ArrowRight, Home, End, Space }
pub enum DialogAnswer { Accept, AcceptWith(String), Dismiss }
pub struct PageRequest { pub method: String, pub address: Address, pub headers: Vec<(String, String)>, pub body: Option<String> }
pub struct PageResponse { pub status: u16, pub address: String, pub headers: Vec<(String, String)>, pub body: String }
pub struct SafeRequests { pub allowed: Vec<SafeRequest> }  // empty by default
pub struct SafeRequest { pub method: String, pub address_prefix: String }

pub struct SnapshotLimits { pub max_elements: usize }      // 400 by default
pub struct Snapshot { pub address: String, pub title: String, pub elements: Vec<SnapshotElement>, pub cut: bool }
impl Snapshot { pub fn to_text(&self) -> String; }         // the text form of the reference document
pub struct SnapshotElement { pub reference: String, pub role: String, pub name: String, pub value: Option<String>,
                             pub states: Vec<ElementState>, pub level: Option<u32>, pub depth: usize }
pub enum ElementState { Disabled, Checked, Expanded, Required, Invalid }
pub const MASK: &str;                                      // what a snapshot shows for a password's value

/// Whether a request reached the page. The crate above decides what that means for a write.
pub enum Dispatch { Sent, NotSent, Unknown }
pub enum Reason { NotVisible, Disabled, NotEditable, Obscured, WrongControl, NoSuchOption }
pub enum LaunchReason { NotInstalled, Supervisor(Failure), NotOwned(String), NoAnswer(String), DataDirectory(Failure) }
pub struct RequestShape { pub method: String, pub address: String }
pub enum BrowserError {
    NotFound, Ambiguous { count: usize }, NotActionable(Reason),
    Timeout { dispatch: Dispatch }, Crashed { dispatch: Dispatch }, Disconnected { dispatch: Dispatch },
    UnexpectedDialog { dispatch: Dispatch }, NotLoaded(String), PasswordField, TooLarge { limit: usize },
    RequestFailed(String), Unexpected(String), LaunchFailed(LaunchReason),
}
impl BrowserError {
    pub fn dispatch(&self) -> Dispatch;           // every error answers
    pub fn failure(&self) -> Option<Failure>;     // the conversions of "Failures" below; None where the crate above must class it
}
pub mod codes { /* this crate's failure codes, and ALL with the class of each */ }

/// Test equipment, behind the feature `test-support` and in this crate's own tests. Never in the product.
pub mod test_support {
    pub const BROWSER_VARIABLE: &str = "A2M_TEST_BROWSER";
    pub const NO_SANDBOX_VARIABLE: &str = "A2M_TEST_BROWSER_NO_SANDBOX";
    pub fn browser_program() -> Option<PathBuf>;
    pub fn settings() -> BrowserSettings;         // the program named, and no sandbox when asked
    pub fn requirement() -> String;               // a `[[requirements]]` entry for a test pack's manifest
}
```

From the kernel: `a2m-provider` hands a pack a `PackContext` when it registers, holding a supervisor scoped to what the pack's manifest declares, the pack's roots as checked paths, the redactor, and telemetry. A context exists only inside `Pack::register`, so `Browsers::new` is called there, by the pack crate built on this one, and the `Browsers` is kept in the objects that pack registers. The web pack is two such crates that must share one set of browsers, so the product crate makes one `SharedBrowsers` cell and hands a clone to each: `get_or_create(&PackContext, BrowserSettings)` builds the `Browsers` for the first caller and returns the same one to the next, and `observer()` gives the discoverers the handle that can only look. Its manifest must declare three things, or `open` is refused: `write_roots = ["profiles"]`; a `[[requirements]]` entry whose `detect.paths` are the paths of `KNOWN_PLACES`; and nothing else for this crate. A path can only be made by `a2m-provider`'s check against the allowed roots; this crate never resolves one.

Contracts that the signatures do not show:

- **The base stays inside.** It is the crate's own protocol client: one reader task, one writer task, requests matched to replies by number, one session per tab on the one connection, and the events of a session handed to that tab's own task in the order they arrived. One private module holds a typed request and reply for each command used and each event listened for; nothing outside it sees untyped JSON.
- **Writing is one task's alone.** A call that is dropped, or passes its deadline, never leaves half a message on the stream, and a request the writer had not reached is never written. That is what lets every failure say `Sent`, `NotSent`, or `Unknown` truthfully.
- **The DevTools connection is a pipe, and a foreign link.** The browser is started with its protocol on an inherited pipe, not a port, as section 204 asks. The protocol is fixed by someone else, so there is no handshake of ours in it: the engine owns a browser by the digest of the program the supervisor started, when settings list known digests, and by its answer to the first request over the pipe, which only its owner holds. A program that does not answer, or speaks another protocol version, is closed. A message is JSON ended by a NUL byte; one that is too large, is not a protocol message, or answers a request that was never made closes the link, and every pending request fails saying whether it had been written. Handing the pipe to the child is the supervisor's: this crate states descriptors 3 and 4, and the Windows argument, as data.
- **A browser is found, never fetched.** A program named in settings, then a fixed list of known places per operating system. No search path, no registry, no environment variable, and no download. A program named in settings that is not there is not replaced by another. Only the program is used; its own profile is never named.
- **The launch arguments are a fixed list.** It never holds `--no-sandbox` or a flag that hides automation, and it does hold `--enable-automation`, which tells the page and a person that the browser is automated. The data directory keeps its own key: the operating system's key store, where a person's own browser keeps its key, is never asked.
- **One browser starts at a time in the whole process,** from the start of the program to its first answer, whichever `Browsers` asks. Browsers that start cold at the same moment each take many times longer than one after the other: four at once on a hosted Windows machine each took more than 30 seconds, and three at once a minute later took under 9. The launch deadline is 120 seconds for the same reason, and a browser that is silent to that deadline is `transient`, `timeout`, not "not a browser": silence proves only slowness.
- **A dialog is answered when the tab has heard it close.** `answer_dialog` returns after the browser's event, not only its reply, so the next call is not told a dialog is still open.
- **The environment is cleared.** The supervisor passes nothing but the few variables an operating system needs to start a program at all, listed as data.
- **A data directory is a checked path:** `profiles/<target>/<environment>/<kind>-<digest of the identity>`, made by joining to the profile root, links followed, before anything starts. An identity can be any text, so its part is a digest.
- **Only what a person can perceive is located.** An element that is not laid out, or is hidden, matches no query. So `Visible` is "at least one match" and `Hidden` is "no match".
- **`locate` does not wait.** It answers for the page as it is. A step that may be early waits for `Visible` first.
- **An element is valid for one page generation and one tab.** A navigation or a re-render that removes the node makes it stale, and using it is `NotFound`, never a silent re-query.
- **A wait is re-armed, not repeated.** A wait that is under way when the document is replaced continues in the new document, woken by the event that replaced the old one. An input and an in-page request are never sent again.
- **The helper runs in an isolated world.** The injected script that locates, checks, and observes cannot be read or changed by the page's own scripts.
- **A method that only looks sends no input,** so each of its errors says `NotSent`, whatever was written to the browser on its way.
- **Everything read is redacted and bounded.** Every text that leaves the crate passes the redactor; a read longer than the limit is `TooLarge`. Cookie, authorization, and set-cookie headers are removed from a `PageResponse` by name.
- **A password input's value is never read.** `read` refuses it, whatever the caller asks, and so does a wait on its text. An input whose `autocomplete` names a password counts as one. A snapshot shows `MASK` for each: the engine learns which nodes they are, not what they hold.

### Two postures

An `ObserveTab` cannot act, because it has no method that acts. That is the first lock. The second is in the browser itself, so that the page's own scripts cannot write either:

| What could change the system | In an observing tab |
| --- | --- |
| A request whose method is not GET, HEAD, or OPTIONS | Intercepted and failed before it leaves the browser, unless it matches the tab's `SafeRequests`, which only a person sets ([section 401](../../spec/4-building-tools/401-overlaying-tools-on-an-existing-system.md)). Each one is reported by `blocked()`. If the answer to the interception is lost, the request stays paused: it is never sent |
| A request from a dedicated worker the tab starts | Passes through the page's own interception, which is on before the page can run; measured, [decision 0111](../../decisions/0111-an-observing-tabs-lock-as-built-and-what-the-browser-was-measured-to-do.md). The worker waits until its sockets are refused, then runs |
| A frame in another process, or any other child | Takes its own interception before it runs. One that cannot take it is not let run, and is reported as `WORKER` |
| A WebSocket, a WebTransport, a peer connection | Refused when the page makes it, and reported. This is a script in the page: a page that sets out to defeat it can |
| A shared worker, a service worker | Refused when the page makes or registers it, and reported; service workers already installed are bypassed |
| A beacon, a form submission | Failed by the same interception |
| Navigation | Only through `goto`, to an address the caller passes |

`a2m-pack-web-probe` is given an `Observer`, so it can hold an `ObserveTab` and never an `ActTab`. The watching tab, a visible window in which a person uses the system while the tab only listens, needs a window and is not built.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-pack-web-ui` | `Browsers::new` inside its `Pack::register`; `Browsers::open`, `ActTab` and `Look` for the `ui`, `page_http`, and `site_tools` drivers; `Visibility::Window`, `export_state`, and `import_state` for its auth providers, once they are built |
| `a2m-pack-web-probe` | `Observer` only: observing tabs, `Look`, `blocked()` |
| `anything2mcp` | Nothing of this crate by name. It registers the web pack, which builds the one `Browsers` and hands its `observer()` to the discoverers |

It calls the scoped supervisor in `a2m-provider` to start and stop browser processes, and `Redact` on every text that leaves it.

## State

| State | Where | Lifetime |
| --- | --- | --- |
| Open browsers, by profile, with tab and run counts | Memory | The engine process |
| A browser's data directory | `profiles/<target>/<environment>/<kind>-<digest>/` in the data directory | Until the session is revoked. Erasing it is not built |
| Known browser digests | Settings | Read at start |

It owns no table. A session's saved state will be sealed by `export_state` and stored by the session keeper, not here.

## Failures

The base returns `BrowserError`. The crates above map it, knowing what the step was. `BrowserError::failure` gives the conversions for errors that mean the same whatever the step, and `None` for the rest:

| Error | Class | Code |
| --- | --- | --- |
| `LaunchFailed(NotInstalled)` | `busy` | `browser_unavailable` |
| `LaunchFailed(Supervisor(_))` | The supervisor's own | For a program the manifest does not declare: `policy`, `effect_not_declared`. Also its limits and its start failures |
| `LaunchFailed(DataDirectory(_))` | The checked path's own | `input`, `path_not_allowed`; `policy`, `effect_not_declared` for a root the manifest does not declare |
| `LaunchFailed(NotOwned(_))`: the pipe ended or carried something else, another protocol version, or an unknown digest | `unknown_variant` | `browser_not_owned` |
| `LaunchFailed(NoAnswer(_))`: the pipe stayed open and nothing answered before the launch deadline | `transient` | `timeout` |
| `Crashed` or `Disconnected` with `NotSent` | `transient` | `connection_lost` |
| `Timeout` with `NotSent` | `transient` | `timeout` |
| `PasswordField` | `policy` | `password_read_refused` |
| `Address::new` refusing a text | `input` | `address_invalid` |

`Crashed`, `Disconnected`, `Timeout`, or `UnexpectedDialog` with `Sent` or `Unknown`, and `RequestFailed`, are never converted here: the driver above decides, and for a commit it is `possible_commit`. `NotFound`, `Ambiguous`, `NotActionable`, `NotLoaded`, `TooLarge`, and `Unexpected` are the driver's to class too: a query that finds nothing is `drift` for a screen model and an answer for a probe.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| A reply reaches the call with its number; an event reaches the subscriber of its session; a deadline ends a call; a malformed or oversized message, or a reply to no request, closes the link and fails everything pending, each saying whether its request was written; a dropped request is never written | Nine tests against a scripted peer on an in-memory stream, with no browser |
| No browser starts with a data directory outside the profile root, or from a program the manifest does not declare | A test with a planted link, through the checked-path constructor; a test with an undeclared program |
| A program that is not a browser, or whose digest is not a known one, is closed and leaves nothing | A test with the test's own program substituted; a test with another digest |
| Two identities never share a process or a data directory | A test that opens two profiles and asks the supervisor |
| An observing tab sends no request other than GET, HEAD, OPTIONS, or a declared safe one, from the page and from a worker, and makes no socket | A test in which the page's own script, and a worker, write: the test application stores the write from an acting tab, and its control interface counts none from an observing tab |
| A late element is noticed without polling; a wait survives a navigation | A test with an element added by a timer of the page, and a page that moves on while the wait is under way |
| An ambiguous query fails; it never picks the first | A test: four inputs match, and the error says four |
| No password, and no value the redactor knows, leaves the crate in a read, a title, or a snapshot | A test with a planted secret and two kinds of password input |
| A write through the page is stored once, with the values sent | A test that fills and saves the test application's form, and asks its control interface |
| A browser killed during a call reports whether the request was sent, and nothing is left | A test that kills the browser while a write is held by the test application: the call says `Sent`, a later call says `NotSent`, the next `open` starts afresh, and no process names the data directory |
| Browsers asked for at once, from several sets, start one after the other and all answer | A test that opens six at once and counts the starts under way |
| With no browser, the failure is `busy`, `browser_unavailable`; a test that needs a browser fails with it and never skips | A test with a program that is not there |
| The launch arguments never hold `--no-sandbox` or a flag that hides automation, and the browser reports itself as automated | A test over the fixed list; a test that asks the page |
| The browser budgets of section 225 hold | Not measured in this crate yet. The spike measured 26 to 33 ms for a warm tab and 0.35 to 0.40 s for a cold start on Linux, headless |

## Tests and the browser they use

The tests of the client run against a scripted peer. The others need a browser and the test application, and run inside the crate, because a few must stand for the page itself.

A test finds its browser as the product does: in the known places. Two variables, read only by the module `test_support`, exist for a machine that differs: `A2M_TEST_BROWSER` names the program by its absolute path, and `A2M_TEST_BROWSER_NO_SANDBOX=1` adds the one argument `--no-sandbox`, for a machine whose tests run as the superuser. The module is compiled only into this crate's tests and into a crate that asks for the feature `test-support` as a dev-dependency. The product never asks for it, so a product build cannot switch the sandbox off ([decision 0110](../../decisions/0110-the-browser-engines-interface-as-building-it-settled-it.md)).

## Build order

1. Done: slice 0.4, in `spikes/browser-base/`, not here: each candidate base against launching, reading, waiting, intercepting, and a killed browser. [Decision 0070](../../decisions/0070-the-browser-engine-stands-on-its-own-client-over-the-debugging-pipe.md), [research note 17](../../research/17-browser-engine-base.md).
2. Done: the typed protocol module, and the protocol client over two byte streams, tested against a scripted peer with no browser.
3. Done: the browser finder; launch through the scoped supervisor with the fixed arguments and a checked data directory; `Browsers::new`, `open`, `list`, `close`, `shutdown`; ownership by digest and by the answer over the pipe. Run on Linux, macOS, and Windows.
4. Done: `ActTab` and `Look`: `goto` on a protocol event, the helper in an isolated world, `locate` by every `By` with exactly one or a failure, `within`, open shadow roots, `read`, every `Primitive`, `snapshot`.
5. Done: `input` with `Dispatch` reported, `answer_dialog`, and `fetch` from inside the page.
6. Done: `Observer` and `ObserveTab` with its interception, the worker rule, and the refusal of sockets.
7. Not built, slice 1.6: declared frames; the check of a helper's match against the browser's own accessibility query; recycling; the stable-position check before a click; `Input::Scroll`; one telemetry span per round trip group.
8. Not built, package 2.3: `Visibility::Window`, `export_state`, `import_state`, the watching tab.
9. Not built, package E2 and later: platform profiles as data; the stream of network exchanges and the `Response` wait; browser events for pop-ups and downloads; uploads; virtualized scrolling; erasing a data directory; a browser installed by `browser.install`.

## Open points

- The Windows handoff of the pipe is proven: the hosted Windows machine started browsers over the two inherited handles once the supervisor connected its ends of the pipes. macOS passed too. On that Windows run the first four browsers, started at the same moment on a cold machine, each took more than the 30 seconds then allowed; hence one start at a time and the longer ceiling. How long one cold start alone takes there has not been measured. The variables passed on Windows are the ones that worked; whether fewer would do is not known.
- The first browser version that takes `--remote-debugging-io-pipes` is not known, so the minimum browser version is not set, and the version a browser reports is not compared with anything.
- The fixed list of known places was written from what the hosted build machines and the common installers use. It has not been collected from a primary source, and per-person installs, which have no fixed path, are not on it.
- The supervisor digests the program before every start. A browser program is about 300 MB, and digesting it is most of a cold start in an unoptimized build. Whether the digest of an unchanged file may be remembered is the supervisor's question.
- The helper computes role and accessible name by the common steps of the naming rules, not all of them. A name it computes differently from the browser makes a field not found, which fails closed; a wrong match is what a fingerprint above is for. Whether the browser's own accessibility query checks every locate is for slice 1.6 and research brief 3.
- The refusal of sockets in an observing tab is a script in the page. A browser-level refusal was not found.
- No session longer than a few seconds has been run, and no memory figure taken. The recycling rule and hypothesis H-01 both need them.
- Whether the browser's sandbox starts on the hosted Linux build machine as it is.
- Fetching and pinning a Chromium build without another project's installer ([research note 09](../../research/09-rust-building-blocks.md)). The `browser.install` action needs it.
- Whether a GET that a page treats as an action can be recognized at this level, or only by the probe's rules above. [Section 402](../../spec/4-building-tools/402-the-scout-and-who-does-what.md) leaves it to Phase 4.
- Whether observing and acting tabs in one browser are safe enough together, or an observing scout should use its own browser on a copy of the profile. Copying a profile is what decision 0032 avoids.
