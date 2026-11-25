# `a2m-pack-web-browser`

The base of the web pack: a browser engine that speaks the DevTools protocol to Chrome, Edge, or Chromium, launched with a data directory the engine owns. It launches and recycles browsers, opens tabs, locates elements by role and name, acts on them, waits on events, observes the network, and reports crashes. It is not a driver and registers nothing; `a2m-pack-web-ui` and `a2m-pack-web-probe` are built on it.

## Facts

| | |
| --- | --- |
| Side | Pack: web |
| Layer | 9, Pack bases |
| May depend on | Layers 1 to 5 |
| Depends on | `a2m-core` for `Failure`, `Redact`, `Digest`, `Availability`, and the names of a profile key; `a2m-provider` for the pack context: the scoped supervisor, checked paths, the redactor, and `Deadline`. Outside: `tokio`, `serde`, and `serde_json`, and nothing else. The base is a DevTools-protocol client of the crate's own ([decision 0070](../../decisions/0070-the-browser-engine-stands-on-its-own-client-over-the-debugging-pipe.md)); there is no outside base library |
| Roadmap packages | Slice 0.4 chose the base. Slice 1.1 built the client, launch, tabs, locating, reading, waiting, snapshots, input, the observing tab, and in-page requests. Package 2.3 the visible window. Package 8.5 the hard cases |
| Carries out | Spec sections [302](../../spec/3-packs/302-the-web-pack-browser-engine.md), [215](../../spec/2-kernel/215-sessions-and-the-session-keeper.md) (profiles and tabs), [204](../../spec/2-kernel/204-supervising-subsystems.md) (for the browser), [225](../../spec/2-kernel/225-performance.md) (the browser budgets); decisions [0004](../../decisions/0004-rust-browser-engine-on-the-devtools-protocol.md), [0070](../../decisions/0070-the-browser-engine-stands-on-its-own-client-over-the-debugging-pipe.md), [0031](../../decisions/0031-the-browser-always-runs-with-a-profile-the-engine-owns.md), [0032](../../decisions/0032-one-live-session-per-identity-shared-by-tabs-with-a-single-renewer.md), [0110](../../decisions/0110-the-browser-engines-interface-as-building-it-settled-it.md), [0111](../../decisions/0111-an-observing-tabs-lock-as-built-and-what-the-browser-was-measured-to-do.md), [0134](../../decisions/0134-one-browser-starts-at-a-time-and-a-silent-start-is-slow-not-foreign.md), [0143](../../decisions/0143-the-browser-engine-after-its-safety-audit.md) |

## Owns

- Launching a browser program, through the supervisor the pack context scopes to the programs its manifest declares, with an engine-owned data directory, one per target, environment, and identity, given as a checked path.
- One browser process per profile, shared by many tabs.
- Tabs with a fixed environment: viewport, locale, time zone, color scheme, reduced motion.
- Resolving a query to exactly one rendered element, through a helper injected into an isolated world, across open shadow roots.
- Actionability checks and input, with the check and the focus in one in-page call.
- Waits that end on a protocol event or an in-page observer, with a deadline. No polling.
- Building a snapshot from the accessibility tree, in the form of [steps, snapshots, and diffs](../../reference/steps-snapshots-and-diffs.md#snapshots), so that a run's evidence and discovery's findings use one builder.
- Requests sent from inside the page.
- The posture of each tab: one that may act, one that may only observe, or one a person uses while the engine watches (below).
- Keeping each tab on the origins it was opened for, holding every request of its page, frames, and workers to those origins and the ones its page may send requests to ([decision 0292](../../decisions/0292-a-web-tab-sends-only-to-its-surface-and-the-destinations-its-tool-declares.md)), and refusing what leaves the page: a download, and a window the page opens.
- Detecting a crash, a disconnect, and a dialog, and saying for each act whether its input reached the page.

Built since ([decision 0187](../../decisions/0187-the-web-pack-matches-a-screen-by-its-address-and-reads-by-key.md)): an observing tab records each request its page makes by itself from a script with GET to one of the tab's origins, as a `PageExchange` of names and kinds only (method, origin, path, the query's names, the names of headers whose name says they carry a credential or an anti-forgery token, status, media type, and a JSON answer's names and kinds to one level), taken with `ObserveTab::exchanges(deadline)`, which waits until no such request is awaiting its answer; the helper gives `dt` and `dd` the roles `term` and `definition`. Built since ([decision 0190](../../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md)): a read from inside an observing tab's page, `ObserveTab::read_from_page`, GET alone to a `PagePath`. Built since ([decision 0195](../../decisions/0195-a-read-never-runs-where-the-page-may-write-and-a-page-keeps-only-its-structure.md)): recording is off until `ObserveTab::record_exchanges` is called, so a tab whose exchanges nobody takes fetches no answer; an answer's body is read off the tab's one stream of events, and the browser keeps none longer than four bytes for each character the tab reads (`Network.enable` with `maxResourceBufferSize`); `exchanges` gives `PageExchanges`, with how many were left out at 256 and how many were still awaited; a `PageExchange` keeps its path by structure (`path_by_structure`: a segment that is not a plain word is `{value}`), only plain names of its query and whether an entry was left out (`query_names`: an entry with no `=` is a value and is never kept), the names of the headers the page's script set (`set_headers`: every header but `BROWSER_HEADERS`, and `accept`, `accept-language`, `user-agent`, and `referer` only when they hold what the browser itself sends), and an answer's names only where they are structure (`names_are_structure`: at most 64 names, each a plain name; otherwise `names_withheld`); the action words and their check (`ACTION_WORDS`, `looks_like_an_action`) live here, for the probe and the drivers alike; a redirect answering `read_from_page` is `BrowserError::Redirected`. Built since ([decision 0350](../../decisions/0350-the-phase-1-cases-of-the-interface-driver-frames-answered-questions-native-dialogs-two-more-controls-and-what-the-grid-still-needs.md)): a query whose `within` is a frame element of the page's own origin is looked for in the frame's document, and a wait watches the frame; a click's middle is given in the page's own coordinates; `By::RowKeyed` and `ReadWhat::Cell` find a row by what its first cell shows and read each cell; `ActTab::open_dialog` says what the open dialog of the browser says, redacted and bounded. Built since ([decision 0351](../../decisions/0351-the-remaining-widget-drivers-built-from-what-a-page-says-of-its-controls-a-staged-file-a-grid-cell-by-its-rows-key.md)): `By::CellKeyed` finds one cell of the row of a key; `ReadWhat::TreePath` and `ReadWhat::Files` read the selected path of a tree and the names of the files of a file input; `Input::Attach` gives a file input one file of a checked path with `DOM.setFileInputFiles`, and nothing else; `Input::Check` and `ReadWhat::Checked` take a switch a page draws with `aria-checked`; `Browsers::staging_folder` is the `staging/` folder of the profile root, as a checked path. Built since ([decision 0362](../../decisions/0362-a-step-that-declares-a-download-lifts-the-refusal-of-downloads-for-itself-alone-into-a-folder-of-the-profile.md)): `ActTab::allow_download`, `downloaded`, and `forbid_downloads` lift the refusal of downloads for one declared step, into `Browsers::downloads_folder`, with a bound on the size and a name the browser makes; the browser writes into a writing folder beside it, and a file is moved in only once it is whole ([decision 0419](../../decisions/0419-a-declared-download-is-written-beside-the-downloads-folder-and-moved-in-once-whole.md)). Built since ([decision 0384](../../decisions/0384-a-screen-changed-on-purpose-is-caught-by-its-locators-or-its-fingerprint-and-a-named-section-is-a-region.md)): the helper gives a `section` with a name of its own the role `region`, and an unnamed one `generic`. Built since ([decision 0391](../../decisions/0391-traffic-capture-is-a-capability-of-the-provider-contract-that-yields-structure-only.md)): `ObserveTab` implements the provider contract's `TrafficCapture` (`start` is `record_exchanges`; `take_recorded` gives `PageExchanges` as `CapturedTraffic`, by `captured`), so a pack that holds observing tabs offers traffic capture through the contract, and what the tab records passes the contract's check on the way out. Built since ([decision 0394](../../decisions/0394-an-asserted-read-of-a-web-surface-is-a-page-request-matched-by-one-shared-shape-and-shown-at-publish.md), point 3, package E7b): a `SafeRequest` is a method, an origin, a path template, and a body (`SafeBody::None`, or a closed `Shape` of the provider contract with `BodyEncoding::Json` or `Form`), in `src/safe.rs`; the interception's `PausedRequest` keeps the body the browser hands it (`HeldBody`, `hasPostData` and `postDataEntries`, printed as a count of parts), decoded only when an observing tab judges a request that one safe request matches by method, origin, and path. A request with a query or a fragment matches none; a request that carries a header overriding its method (`METHOD_OVERRIDES`) is failed in an observing tab whatever its method, a `GET`, `HEAD`, or `OPTIONS` among them, since a system that honours the header reads it as the method it names; a body is failed when the browser says there is one and gives no bytes for a part of it, past `LONGEST_SAFE_BODY`, of another `Content-Type` or charset, not UTF-8, not parsing in its encoding or naming one member twice, or not fitting the shape. Measured on Chrome for Testing 153 on Linux: a text, `URLSearchParams`, a `FormData` of texts and of in-memory files, a `Blob`, and an `ArrayBuffer`, sent by `fetch`, `XMLHttpRequest`, or `sendBeacon`, arrive whole in `postDataEntries`, two mebibytes among them; a `ReadableStream` arrives as one part with no bytes; an empty body arrives as none; `postData` drops bytes that are not UTF-8 and is never read. A file chosen from disk was not measured; a part with no bytes fails whatever it is. Built since ([decision 0392](../../decisions/0392-a-page-that-reads-by-a-method-that-may-write-is-learned-from-a-persons-watched-use-and-read-through-an-asserted-request.md), point 1, package E2c): the watching tab, below under "The watching tab"; a `PageExchange` keeps the body a request sent as `body_media_type`, `body_fields`, and `body_names_withheld`, which only a watching tab fills. Built since ([decision 0422](../../decisions/0422-a-tab-that-answers-not-even-a-probe-is-closed-and-three-hangs-recycle-its-browser-between-runs.md), package 9.3f): a tab of the engine's whose call ends at its deadline unanswered, or is dropped before its reply, or whose `goto` passes its deadline before its document is parsed, is probed on a task of its own: a constant asked of the page within `BrowserSettings::hang_probe` (`HANG_PROBE`, 5 s). A page that answers keeps its tab; one that does not is hung: its script is ended (`Runtime.terminateExecution`, since a renderer held by its own script outlives its closed tab, measured on Chrome for Testing 153), its page and windows closed, and every call on it since is `Timeout` with nothing sent. A sign-in tab and a watching tab are never probed. Each hang is counted against the browser (`OpenBrowser::hangs`); at `BrowserSettings::hangs_before_recycle` (`HANGS_BEFORE_RECYCLE`, 3) the next `Browsers::open` that finds no tab of it held closes it as `close` does and starts it afresh, so a browser is recycled between runs and never inside one. Built since ([decision 0424](../../decisions/0424-a-read-whose-browser-is-lost-goes-on-in-a-new-browser-of-the-same-profile-and-only-its-observing-tab-is-opened-again.md), package 9.3e): `ObserveTab::browser_lost` says whether the engine's link to the tab's browser is closed, so that the crate above can open the tab again in a new browser of the same profile. Built since ([decision 0425](../../decisions/0425-a-browsers-memory-is-measured-from-its-own-list-of-its-processes-and-past-a-ceiling-no-new-browser-work-starts.md), package 9.3g): an open that begins browser work (of a new browser, of one none of whose tabs is held, or of a window) measures every open browser that still runs: the browser names its processes (`SystemInfo.getProcessInfo`), and `src/memory.rs` measures each by its operating system (Linux, `Pss` of `/proc/<id>/smaps_rollup`; macOS, `ri_phys_footprint` of `proc_pid_rusage`; Windows, `PrivateUsage` of `GetProcessMemoryInfo`), a number that names no process holding nothing. The profile's own browser past `BrowserSettings::memory_limit` (`MEMORY_LIMIT`, 2 GiB), or unmeasured, is recycled as after its hangs, between runs and never inside one; then, while the browsers measured hold `BrowserSettings::memory_ceiling` (`MEMORY_CEILING`, 4 GiB) or more, or one is unmeasured, the open starts nothing and fails `LaunchFailed(MemoryShort)`, `busy`/`memory_short` with `memory_retry` (`MEMORY_RETRY`, 30 s) as its retry time. An open that finds a tab of its profile's browser held is a run's work going on, and is not measured. Designed and not built: recycling by run count and age; frames of another origin; platform profiles as data; the stream of every network exchange; pop-ups and downloads a step declares (an undeclared one is refused and reported).

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
    pub hangs_before_recycle: u32,        // 3: hung tabs before the browser is recycled between runs (decision 0422)
    pub hang_probe: Duration,             // 5 s: how long a page that did not answer is given to answer a probe
}
impl Default for BrowserSettings;
impl BrowserSettings { pub fn with_program(self, PathBuf) -> Self; pub fn with_known_digests(self, Vec<Digest>) -> Self;
    /// Reads `browser_digests`, a list of digests, from a pack's settings group. A value it cannot read is refused.
    pub fn with_pack_settings(self, settings: &Values) -> Result<Self, Failure>; }
pub const DIGESTS_SETTING: &str = "browser_digests";   // the key a pack's manifest declares under `[[settings]]`, type `list`
pub const HANGS_BEFORE_RECYCLE: u32;          // 3: the default of `hangs_before_recycle`
pub const HANG_PROBE: Duration;               // 5 s: the default of `hang_probe`

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
#[non_exhaustive] pub enum Visibility { Background, Window { notice: String } }   // a window: always started afresh (decision 0254)
pub enum BrowserHealth { Idle, Busy, Exited }
pub struct OpenBrowser { pub profile, pub subsystem: SubsystemId, pub product: String,
                         pub program_digest: Digest, pub verified: bool, pub tabs: usize,
                         pub runs: u64, pub hangs: u64, pub health: BrowserHealth }

pub struct Browser;                                        // cheap to clone; one process
impl Browser {
    pub async fn acting_tab(&self, env: &PageEnvironment, origins: &Origins) -> Result<ActTab, BrowserError>;
    pub fn health(&self) -> BrowserHealth;
    pub fn product(&self) -> &str;                         // what the browser reports itself to be
    pub fn subsystem(&self) -> SubsystemId;
    /// The cookies of the browser's context for `origins`' hosts (decision 0245). Values are `Secret`s.
    pub async fn export_state(&self, origins: &Origins, deadline: Deadline) -> Result<BrowserState, BrowserError>;
    /// Places them, unless the state of the same `mark` was placed already. True when placed now.
    /// A cookie for none of `origins`' hosts refuses the whole state (`ForeignOrigin`; decision 0254).
    pub async fn import_state(&self, mark: &str, state: &BrowserState, origins: &Origins, deadline: Deadline) -> Result<bool, BrowserError>;
    /// A tab a person signs in in: held to both lists, its helper looking at `look_on` alone, no network event.
    pub async fn sign_in_tab(&self, env: &PageEnvironment, look_on: &Origins, pass_through: &Origins) -> Result<ActTab, BrowserError>;
    pub fn shows_window(&self) -> bool;
}
pub struct BrowserState { pub cookies: Vec<StateCookie> }   // Debug shows names only; not serializable
pub struct StateCookie { pub name, pub value: Secret, pub domain, pub path, pub expires: Option<serde_json::Number>,
                         pub http_only: bool, pub secure: bool, pub same_site: Option<String> }

pub struct Observer;                                       // a `Browsers` that can only look
impl Observer {
    pub async fn observing_tab(&self, profile: &ProfileKey, env: &PageEnvironment, allow: &SafeRequests,
        origins: &Origins) -> Result<ObserveTab, BrowserError>;
    /// A tab a person uses in a window while the engine watches (decision 0392, point 1): sent once to `start`, nowhere after.
    pub async fn watching_tab(&self, profile: &ProfileKey, env: &PageEnvironment, notice: &str, origins: &Origins,
        start: &WatchStart, deadline: Deadline) -> Result<WatchTab, BrowserError>;
    pub fn availability(&self) -> Availability;
}
/// The first address of a watching tab: on the origins, and not looking like an action (decision 0144).
pub struct WatchStart;
impl WatchStart { pub fn new(address: &Address, origins: &Origins) -> Result<Self, Failure>; pub fn address(&self) -> &Address; }
pub struct WatchTab;                                       // no Look, no input, no fetch; closed with its window when dropped
impl WatchTab {
    pub fn blocked(&self) -> Vec<RequestShape>;            // NAVIGATION, OUTSIDE, POPUP, DOWNLOAD, a socket: never a method
    pub async fn exchanges(&self, deadline: Deadline) -> PageExchanges;   // every method, with the body by structure
    pub fn screens(&self) -> WatchedScreens;               // a snapshot of each screen, no value kept, taken out
    pub fn changes(&self) -> u64; pub async fn changed_since(&self, seen: u64, deadline: Deadline) -> Result<(), BrowserError>;
    pub fn looked_at(&self) -> usize; pub async fn looked_at_since(&self, seen: usize, deadline: Deadline) -> bool;
    pub fn is_open(&self) -> bool; pub async fn closed(&self, deadline: Deadline) -> bool;   // the person closed it
}
impl TrafficCapture for WatchTab { /* records from the moment it opens; take_recorded: exchanges, as CapturedTraffic */ }
pub struct WatchedScreens { pub snapshots: Vec<Snapshot>, pub bound_hit: bool, pub missed: usize }
pub const LONGEST_WATCHED_BODY: usize;   // 64 KiB: a longer body is recorded by its media type alone
pub const SCREENS_KEPT: usize;           // 40
pub const SETTLE_BOUND: Duration;        // 10 s: how long a screen waits for its requests to end

pub struct PageEnvironment { pub viewport: (u32, u32), pub locale: String, pub time_zone: String,
                             pub dark: bool, pub reduced_motion: bool }    // has a Default
pub struct Address;                                        // an absolute http or https address
impl Address { pub fn new(text: &str) -> Result<Self, Failure>; pub fn as_str(&self) -> &str;
                pub fn origin(&self) -> String; }         // as the browser writes it: `https://app.example.com`
/// The origins a tab may be on: given when it is opened, from the compiled target's destinations. Empty: nowhere.
pub struct Origins;
impl Origins { pub fn of<'a>(addresses: impl IntoIterator<Item = &'a Address>) -> Self;
                pub fn and(self, more: &Origins) -> Self;
                /// Where the page may send requests, the tab never being on them (decision 0292).
                pub fn and_requests_to<'a>(self, addresses: impl IntoIterator<Item = &'a Address>) -> Self;
                pub fn allows(&self, address: &Address) -> bool; pub fn iter(&self) -> impl Iterator<Item = &str>;
                pub fn requests_only(&self) -> impl Iterator<Item = &str>;
                /// The network place of each origin a request may go to, with the reason (decision 0313). The guard does not consult it.
                pub fn places(&self, state: &MachineState) -> Vec<(&str, Placed)>; }
/// What `blocked()` reports for a request to an origin outside the tab's reach.
pub const OUTSIDE: &str = "OUTSIDE";

/// Everything that only looks. Both tab kinds have it. Each method returns a future that is Send.
pub trait Look {
    async fn goto(&self, address: &Address, deadline: Deadline) -> Result<Loaded, BrowserError>;
    async fn locate(&self, query: &Query, deadline: Deadline) -> Result<Element, BrowserError>;
    async fn read(&self, element: &Element, what: &ReadWhat, deadline: Deadline) -> Result<RawValue, BrowserError>;
    async fn wait(&self, until: &Primitive, deadline: Deadline) -> Result<(), BrowserError>;
    /// Whether it holds now: the page looks once and answers at once; `false` is only the page's
    /// own word. The deadline bounds the browser's answer alone: no answer is `Timeout`, never `false`.
    async fn holds(&self, condition: &Primitive, deadline: Deadline) -> Result<bool, BrowserError>;
    async fn snapshot(&self, limits: &SnapshotLimits, deadline: Deadline) -> Result<Snapshot, BrowserError>;
}
pub struct ActTab;     impl Look for ActTab;       // closed when dropped
pub struct ObserveTab; impl Look for ObserveTab;   // closed when dropped
impl ActTab {
    pub async fn input(&self, element: &Element, how: &Input, deadline: Deadline) -> Result<(), BrowserError>;
    pub async fn fetch(&self, request: &PageRequest, deadline: Deadline) -> Result<PageResponse, BrowserError>;
    pub async fn answer_dialog(&self, how: &DialogAnswer, deadline: Deadline) -> Result<(), BrowserError>;
    /// A write from inside the page, with the page's own token read and sent in the same call (decision 0206).
    pub async fn write_from_page(&self, write: &PageWrite, deadline: Deadline) -> Result<PageAnswer, BrowserError>;
    /// Where the tab's document is, and the origins of its frames, by the browser's word (decision 0254).
    pub async fn address(&self, deadline: Deadline) -> Result<String, BrowserError>;
    pub async fn frame_origins(&self, deadline: Deadline) -> Result<Vec<String>, BrowserError>;
    /// A count of documents loaded and refusals; a wait for it to grow; a wait for a condition that ends on a change.
    pub fn changes(&self) -> u64;
    pub async fn changed_since(&self, seen: u64, deadline: Deadline) -> Result<(), BrowserError>;
    pub async fn wait_unless_changed(&self, until: &Primitive, seen: u64, deadline: Deadline) -> Result<bool, BrowserError>;
}
pub enum WriteMethod { Post, Put, Patch, Delete }          // the closed list; GET is the read's
pub enum TokenFrom { Meta(String), Input(String), Cookie(String) }  pub enum TokenTo { Header(String), Field(String) }
pub struct AntiForgery { pub from: TokenFrom, pub to: TokenTo }
pub enum FieldValue { Text(String), Whole(i64), Truth(bool) }  pub enum BodyEncoding { Json, Form }
impl PageWrite { pub fn new(method: WriteMethod, path: PagePath, encoding: BodyEncoding,
    fields: BTreeMap<String, FieldValue>, token: Option<AntiForgery>) -> Result<Self, Failure>; }  // every name plain; no header the browser sets
pub enum TokenProblem { Missing, Several, Unusable }       // BrowserError::Token(_): nothing was sent
impl ActTab { pub fn blocked(&self) -> Vec<RequestShape>; }       // NAVIGATION, POPUP, DOWNLOAD, OUTSIDE, a socket: what it refused, taken out
impl ObserveTab { pub fn blocked(&self) -> Vec<RequestShape>; }   // those, and every request and socket it refused
impl ObserveTab { pub fn browser_lost(&self) -> bool; }           // the link to its browser is closed (decision 0424)
impl TrafficCapture for ObserveTab { /* start: record_exchanges; take_recorded: exchanges, as CapturedTraffic (decision 0391) */ }
impl ObserveTab { pub fn record_exchanges(&self); pub async fn exchanges(&self, deadline: Deadline) -> PageExchanges;
    /// GET alone, to `path` on the page's own origin, with no header and no body; no redirect followed.
    pub async fn read_from_page(&self, path: &PagePath, deadline: Deadline) -> Result<PageAnswer, BrowserError>; }
pub fn plain_segment(text: &str) -> bool;                  // letters, digits, `-`, `.`, `_`, `~`; not dots only; at most 512 bytes
pub const MAX_SEGMENT_BYTES: usize;                         // 512
pub struct PagePath;                                        // a pattern with each `{placeholder}` one plain segment
impl PagePath { pub fn fill(pattern: &str, values: &BTreeMap<String, String>) -> Result<Self, Failure>; pub fn as_str(&self) -> &str; }
pub struct PageAnswer { pub status: u16, pub media_type: String, pub body: String,
                        pub withheld: bool }   // no header; the body redacted; withheld: a write's answer quoted its token

pub struct Loaded { pub address: String, pub title: String, pub status: Option<u16> }
pub struct Element;                                        // one element of one page generation and one tab
pub enum By { RoleName { role, name }, Label { text }, Placeholder { text }, Text { text, contains },
              TestId { id }, Title { text }, Css { selector } }
pub struct Query { pub by: By, pub within: Option<Box<Query>> }
impl Query { pub fn new(by: By) -> Self; pub fn role(role: &str, name: &str) -> Self; pub fn within(self, scope: Query) -> Self; }
pub struct TextMatch { pub text: String, pub contains: bool }
pub enum Primitive { Visible(Query), Hidden(Query), Enabled(Query), Text(Query, TextMatch),
                     Url(TextMatch), All(Vec<Primitive>), Any(Vec<Primitive>) }
pub enum ReadWhat { Value, Text, Checked, SelectedText, Attribute { name: String }, Cell { index: usize }, TreePath, Files }
pub enum RawValue { Text(String), Flag(bool), Absent }
pub enum Input { Fill(String), Type(String), NativeSet(String), Blur, Press(Key), Click, Check(bool), SelectByValue(String), Attach(PathBuf) }
pub enum Key { Enter, Tab, Escape, Backspace, Delete, ArrowUp, ArrowDown, ArrowLeft, ArrowRight, Home, End, Space }
pub enum DialogAnswer { Accept, AcceptWith(String), Dismiss }
pub struct PageRequest { pub method: String, pub address: Address, pub headers: Vec<(String, String)>, pub body: Option<String> }
pub struct PageResponse { pub status: u16, pub address: String, pub headers: Vec<(String, String)>, pub body: String }
pub struct SafeRequests { pub allowed: Vec<SafeRequest> }  // empty by default
pub struct SafeRequest { pub method: String, pub origin: String, pub path: String, pub body: SafeBody }
    // origin as the browser writes it; path a template, a `{name}` segment matching one segment
pub enum SafeBody { None, Shaped { encoding: BodyEncoding, shape: Shape } }   // the provider contract's `shape::Shape`
    // the shape's top level a closed object; a form's values are texts
pub const LONGEST_SAFE_BODY: usize;   // 1 MiB

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
pub enum LaunchReason { NotInstalled, NoScreen, Supervisor(Failure), NotOwned(String), NoAnswer(String), DataDirectory(Failure) }
pub struct RequestShape { pub method: String, pub address: String }
pub enum BrowserError {
    NotFound, Ambiguous { count: usize }, NotActionable(Reason),
    Timeout { dispatch: Dispatch }, Crashed { dispatch: Dispatch }, Disconnected { dispatch: Dispatch },
    UnexpectedDialog { dispatch: Dispatch }, NotLoaded(String), PasswordField, ForeignOrigin(String),
    TooLarge { limit: usize }, RequestFailed(String), Unexpected(String), LaunchFailed(LaunchReason),
}
impl BrowserError {
    pub fn dispatch(&self) -> Dispatch;           // every error answers
    pub fn failure(&self) -> Option<Failure>;     // the conversions of "Failures" below; None where the crate above must class it
}
pub mod codes { /* this crate's failure codes in one failure_codes! table (decision 0258); TIMEOUT and CONNECTION_LOST are the provider contract's */ }

/// Test equipment, behind the feature `test-support` and in this crate's own tests. Never in the product.
pub mod test_support {
    pub const BROWSER_VARIABLE: &str = "A2M_TEST_BROWSER";
    pub const NO_SANDBOX_VARIABLE: &str = "A2M_TEST_BROWSER_NO_SANDBOX";
    pub fn browser_program() -> Option<PathBuf>;
    pub fn settings() -> BrowserSettings;         // the program named, and no sandbox when asked
    pub fn requirement() -> String;               // a `[[requirements]]` entry for a test pack's manifest
    pub fn launches_under(folder: &Path) -> u64;  // browser programs this process started for profile roots inside `folder`
}
```

From the kernel: `a2m-provider` hands a pack a `PackContext` when it registers, holding a supervisor scoped to what the pack's manifest declares, the pack's roots as checked paths, the redactor, and telemetry. A context exists only inside `Pack::register`, so `Browsers::new` is called there, by the pack crate built on this one, and the `Browsers` is kept in the objects that pack registers. The web pack is two such crates that must share one set of browsers, so the product crate makes one `SharedBrowsers` cell and hands a clone to each: `get_or_create(&PackContext, BrowserSettings)` builds the `Browsers` for the first caller and returns the same one to the next, and `observer()` gives the discoverers the handle that can only look. Its manifest must declare three things, or `open` is refused: `write_roots = ["profiles"]`; a `[[requirements]]` entry whose `detect.paths` are the paths of `KNOWN_PLACES`; and nothing else for this crate. A path can only be made by `a2m-provider`'s check against the allowed roots; this crate never resolves one.

Contracts that the signatures do not show:

- **The base stays inside.** It is the crate's own protocol client: one reader task, one writer task, requests matched to replies by number, one session per tab on the one connection, and the events of a session handed to that tab's own task in the order they arrived. One private module holds a typed request and reply for each command used and each event listened for; nothing outside it sees untyped JSON.
- **Writing is one task's alone.** A call that is dropped, or passes its deadline, never leaves half a message on the stream, and a request the writer had not reached is never written. That is what lets every failure say `Sent`, `NotSent`, or `Unknown` truthfully.
- **The DevTools connection is a pipe, and a foreign link.** The browser is started with its protocol on an inherited pipe, not a port, as section 204 asks. The protocol is fixed by someone else, so there is no handshake of ours in it: the engine owns a browser by the digest of the program the supervisor started, when settings list known digests, and by its answer to the first request over the pipe, which only its owner holds. A program that does not answer, or speaks another protocol version, is closed. A message is JSON ended by a NUL byte; one that is too large, is not a protocol message, or answers a request that was never made closes the link, and every pending request fails saying whether it had been written. Handing the pipe to the child is the supervisor's: this crate states descriptors 3 and 4, and the Windows argument, as data.
- **A browser is found, never fetched.** A program named in settings, then a fixed list of known places per operating system. No search path, no registry, no environment variable, and no download. A program named in settings that is not there is not replaced by another. Only the program is used; its own profile is never named.
- **The launch arguments are a fixed list.** It never holds `--no-sandbox` or a flag that hides automation, and it does hold `--enable-automation`, which tells the page and a person that the browser is automated. The data directory keeps its own key: the operating system's key store, where a person's own browser keeps its key, is never asked. Every connection is direct: `--no-proxy-server` outranks a proxy in the environment, in the operating system's settings (automatic discovery included), and in the profile's stored settings, and `--disable-extensions` keeps any extension, one another program installs into every profile included, from setting a proxy that would outrank it ([decision 0201](../../decisions/0201-a-resume-is-never-lost-the-browser-takes-no-proxy-and-no-product-build-speaks-a-stands-protocol.md)). A machine policy that sets the browser's proxy outranks every launch argument; the launch cannot close it, and it is the machine administrator's.
- **One browser starts at a time in the whole process,** from the start of the program to its first answer, whichever `Browsers` asks. Browsers that start cold at the same moment each take many times longer than one after the other: four at once on a hosted Windows machine each took more than 30 seconds, and three at once a minute later took under 9. The launch deadline is 120 seconds for the same reason, and a browser that is silent to that deadline is `transient`, `timeout`, not "not a browser": silence proves only slowness. ([decision 0134](../../decisions/0134-one-browser-starts-at-a-time-and-a-silent-start-is-slow-not-foreign.md))
- **A dropped launch leaves nothing behind.** The engine drops a driver's preparation that passes the time the web pack's manifest states for it, three minutes, which is longer than the launch deadline and a tab's opening together, so a slow start is judged by the launch deadline and not cut short ([decision 0427](../../decisions/0427-a-mode-states-how-long-its-prepare-may-take-and-the-engine-honours-it-under-a-ceiling.md)); a person who raises `launch_deadline` past it is cut short again. The driver contract says that drop must be safe, and it still happens: on that time, and when the run itself is ended. A browser whose program started and that is not yet recorded among the open ones is held by a guard: dropped there, the guard begins its stop at once and lists it, and the next launch and `shutdown` finish that stop before anything else, so no subsystem keeps the profile's place in the supervisor and no later start is refused `busy`, `subsystem_limit`.
- **A dialog is answered when the tab has heard it close.** `answer_dialog` returns after the browser's event, not only its reply, so the next call is not told a dialog is still open.
- **The environment is cleared.** The supervisor passes nothing but the few variables an operating system needs to start a program at all, listed as data.
- **A data directory is a checked path:** `profiles/<target>/<environment>/<kind>-<digest of the identity>`, made by joining to the profile root, links followed, before anything starts. An identity can be any text, so its part is a digest.
- **Only what a person can perceive is located.** An element that is not laid out, or is hidden, matches no query. So `Visible` is "at least one match" and `Hidden` is "no match".
- **`locate` does not wait.** It answers for the page as it is. A step that may be early waits for `Visible` first.
- **An element is valid for one page generation and one tab.** A navigation or a re-render that removes the node makes it stale, and acting on it is `NotFound`, never a silent re-query. A read alone finds it again, in the same document and the same evaluation that reads it, by the `Query` the `Element` keeps, when that query matches exactly one element; otherwise it is `NotFound` ([decision 0379](../../decisions/0379-a-read-of-an-element-the-page-drew-again-finds-it-by-the-query-that-found-it.md)).
- **A wait is re-armed, not repeated.** A wait that is under way when the document is replaced continues in the new document, woken by the event that replaced the old one. An input and an in-page request are never sent again.
- **The helper runs in an isolated world.** The injected script that locates, checks, and observes cannot be read or changed by the page's own scripts.
- **A method that only looks sends no input,** so each of its errors says `NotSent`, whatever was written to the browser on its way.
- **Everything read is redacted and bounded.** Every text that leaves the crate passes the redactor; a read longer than the limit is `TooLarge`. Cookie, authorization, and set-cookie headers are removed from a `PageResponse` by name.
- **Redaction comes before any cut.** A message, a refused address, and an origin taken out of a longer text are redacted whole and only then cut to length or at the query. Cut first, a secret that the cut divides would leave its first part behind.
- **A password input's value is never read.** `read` refuses it, whatever the caller asks, and so does a wait on its text. An input whose `autocomplete` names a password counts as one. A snapshot shows `MASK` for each: the helper lists the ones it sees before the accessibility tree is taken, and after it the browser itself is asked about every node that carries a value, which also reaches into a closed shadow root. A text input that a page switched from a password, and that names none in `autocomplete`, is not recognized; only the redactor knows that value.
- **A selector cannot ask what a password was served with.** On a page where a password input carries a `value` attribute, a `css` query that asks about that attribute, or holds an escape that could spell its name, is `PasswordField`, in a locate and in a wait. A right guess and a wrong one are refused alike.
- **A tab's page sends only where the tab may reach** ([decision 0292](../../decisions/0292-a-web-tab-sends-only-to-its-surface-and-the-destinations-its-tool-declares.md)). In both postures every request of the page, its frames, and its workers, whatever its method and resource type, is held, and one whose address is on neither the tab's origins nor those `and_requests_to` added is failed before it leaves the browser and reported as `OUTSIDE` with its address cut at the query. An acting tab holds every request as an observing tab does, attaches each frame in another process and each worker waiting, and lets it run only once it holds its own requests and carries the socket script; a WebSocket or WebTransport to an origin outside the reach, a peer connection, a shared worker, and a service worker's registration are refused by that script. A service worker takes the observing tab's stricter script in either posture. A sign-in tab holds its documents and the requests of the kinds this browser's interception filter names that carry no body (a style, an image, media, a font, a script, `Other`), to its origins and the providers', and reports one outside as `OUTSIDE`; it does not hold a `fetch`, an `XHR`, a beacon, or a socket ([decision 0335](../../decisions/0335-a-scout-carries-the-origins-a-page-tried-and-a-surface-may-refuse-origins-quietly.md)). `Origins::and_refused_quietly` adds origins a request to which is failed and not reported; it wins over every other set.
- **A tab stays on the origins it was opened for.** `Origins` is given when the tab is opened and never changes. Three things hold it, and each would hold without the others: `goto` and `fetch` refuse an address of another origin before the browser hears of it; the browser ends the request of the tab's next document when it is for another origin, whoever started the move and at every redirect; and the helper checks the page's origin in the same in-page call as every locate, read, wait, check, focus, and in-page request, so the check falls after the element is located and before any input is sent. Each refusal is `ForeignOrigin`, and nothing was sent. An origin is compared as text with what the browser reports, so a host written in a form the browser rewrites matches nothing: a difference can only close the tab's reach.
- **What leaves the page is refused, and reported.** Every download is denied for the whole browser when it starts: no file is written, and the tab reports `DOWNLOAD`. A window a page opens is held before it runs, every request of it is ended, and it is closed; the tab that opened it reports `POPUP`. Both hold in an acting tab too, where a step that declares one is still to be built. Every permission is refused for every origin when the browser starts, so no page is granted one and none waits on a prompt: left to itself, the headless browser granted a page the clipboard, sensors, and the background sync and fetch by which a service worker sends requests after its page has gone. A file chooser opens nothing, and the page goes on. A browser that does not take the refusal of downloads or of permissions is closed as not owned.
- **An address that is kept carries no query.** A snapshot's address, the address of a `PageResponse`, and every address `blocked()` reports are cut at the query, where tokens travel. `Loaded.address` keeps its query, redacted, because the crates above decide by it; it is not for showing.
- **An in-page request follows no redirect.** A redirect's next address was checked by nobody, so the answer is `RequestFailed` and what the first request did is for the crate above to find out. `page_http` follows none either.
- **A read from inside the page cannot send anything but a read** ([decision 0190](../../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md)). `read_from_page` takes no method, header, body, or origin: the helper makes the address from the page's own origin and a `PagePath`, refuses a page with no origin, checks the page's origin against the tab's in the same call, fetches with GET, the page's cookies of the same origin and nothing more, and stops reading past `max_read_chars` (`TooLarge`). A value that is not one plain segment is refused by `PagePath::fill`, `input`, `address_value_refused`, and nothing is sent. Only an observing tab has the call, so its guard still fails any request of the page that could write; a test sends POST from the same page and the application sees none. No credential is read out of the page: the answer's headers are not returned.
- **A write from inside the page is an acting tab's, and its token never leaves the page** ([decision 0206](../../decisions/0206-the-page-http-mode-writes-by-put-with-the-pages-own-token-in-an-acting-tab.md)). `write_from_page` is on `ActTab` alone; an observing tab has none, and its browser fails the request beneath it. The method is from a closed list, the path a `PagePath`, every name of a field, a header, or a token's place plain, and the one header that may be named is the token's, never one the browser or the connection sets. The helper finds the token where the write says (a `meta` by name, a hidden `input` by name, or a cookie the page's script may read), refuses before `fetch` a token missing, two different ones, or one not of a token's form (`BrowserError::Token`, not sent), sends once with the page's cookies of the same origin and no redirect followed, replaces every copy of the token in the answer before the answer leaves the page, and hands back no text of an error. The browser itself may send the request again when the connection closed unanswered: measured, a PUT was sent twice.
- **What is typed is not printed.** The debug form of `Input` gives the length of a text, and that of `PageRequest` the names of its headers and the length of its body. The fields are plain text all the same: the crates above hold a password in a `String` until it is typed.
- **A data directory is its owner's alone.** On Linux and macOS it is set to mode 0700 before the browser starts on it, and one that is still open to others afterwards is refused; the browser's own files are 0600 and 0700, measured. On Windows the folder takes its parent's access list and the engine reads none.
- **Nothing a page stores is written to the data directory** ([decision 0245](../../decisions/0245-a-web-application-is-signed-in-to-through-its-own-sign-in-page-with-a-credential-stored-for-the-person.md)). The browser makes a context of its own when it starts (`Target.createBrowserContext`), every tab is opened in it, and downloads are denied and permissions refused for it as for the default context. It keeps cookies and page storage in memory: a test signs in through the test application's page, closes the browser, and finds the session cookie in no file. A browser that does not make the context is closed as not owned. A session's state leaves the browser only through `export_state`, as `Secret`s, and returns through `import_state`, once for each mark, so a cookie the application changed since is kept.
- **The page's service worker is passed by in both kinds of tab** (decision 0245, audit finding M6), so a write from inside the page reaches the application as sent, and no worker answers it.
- **A write's answer that quotes its token in any spelling is withheld whole in the page** (decision 0245, audit finding S4): after JSON escapes, character references, and percent-encoding are undone, in any case, the token or its first half is looked for; on a match `PageAnswer::withheld` is set and the body is empty. Measured ([decision 0254](../../decisions/0254-a-person-signs-in-in-a-window-of-the-engines-own-browser-and-the-web-pack-stops-on-every-declared-sign-of-expiry.md), audit finding M7): an acting tab's network events cannot be switched off, since the service worker is passed by only while they are on; they carry no body of a request the page's script makes (`maxPostDataSize` of one byte). A form a document posts, the cookies, and a token in a header still cross, in the events the record names; and since decision 0292 the body of every request the page's script makes crosses in the pause of that request (`Fetch.requestPaused`), which holding the request to the tab's reach needs (measured). None of those fields is parsed or kept in an acting tab; an observing tab decodes the pause's body only to judge a request that one safe request matches by method, origin, and path, and keeps nothing of it ([decision 0394](../../decisions/0394-an-asserted-read-of-a-web-surface-is-a-page-request-matched-by-one-shared-shape-and-shown-at-publish.md)). A sign-in tab hears no network event.
- **Looking at the disk for a launch is off the async threads:** finding the program and making the folder run on a blocking thread. `availability()` is not async and looks when it is called.

### Two postures

An `ObserveTab` cannot act, because it has no method that acts. That is the first lock. The second is in the browser itself, so that the page's own scripts cannot write either. Each row was tried with a page of the test application that attempts it ([decision 0143](../../decisions/0143-the-browser-engine-after-its-safety-audit.md)):

| What could change the system | In an observing tab |
| --- | --- |
| A request whose method is not GET, HEAD, or OPTIONS | Intercepted and failed before it leaves the browser, unless it matches one of the tab's `SafeRequests` in method, origin, path template, and body, which only a person sets ([section 401](../../spec/4-building-tools/401-overlaying-tools-on-an-existing-system.md)). Each one is reported by `blocked()`. If the answer to the interception is lost, the request stays paused: it is never sent |
| A request from a dedicated worker the tab starts | Passes through the page's own interception, which is on before the page can run; measured, [decision 0111](../../decisions/0111-an-observing-tabs-lock-as-built-and-what-the-browser-was-measured-to-do.md). The worker waits until its sockets are refused, then runs |
| A frame in another process | Takes its own interception before it runs, and the refusal of sockets on every document it loads. One that cannot take them is not let run, and is reported as `WORKER` |
| A service worker of the page's origin | Attached under the page, also when it was already running: it takes its own interception and the refusal of sockets |
| A WebSocket, a WebTransport, a peer connection | Refused when the page makes it, and reported. This is a script in the page: a page that sets out to defeat it can |
| A shared worker, a service worker | Refused when the page makes or registers it, and reported, also from a frame the page makes for the purpose. An acting tab's page cannot make or register one either (decision 0292). The page's own requests pass by a service worker already installed; one woken all the same is locked, as the row above says |
| A request to an origin outside the tab's reach | Failed before it leaves the browser, whatever its method, and reported as `OUTSIDE`, in this posture and in an acting tab (decision 0292) |
| A permission, a file chooser | Every permission refused in the browser, and no chooser opened, as in an acting tab |
| A beacon, a form submitted with POST | Failed by the same interception. The page stays as it was |
| Navigation | Only through `goto`, to an address the caller passes, on one of the tab's origins, and through its redirects on those origins. A move the page starts, by script, a refresh tag, or a form, is ended before its request leaves the browser, and reported as `NAVIGATION`. A move through the tab's history makes no request and is not ended: it shows a page the tab was already sent to |
| A download | Denied for the whole browser: no file is written. Reported as `DOWNLOAD` |
| A window the page opens | Held before it runs, every request of it ended, and closed. Reported as `POPUP` |

In an `ActTab` the page may write, and these rows still hold: the tab's own document is held to the tab's origins, a request outside the tab's reach is failed, a frame in another process and a worker are locked before they run, a socket outside the reach, a peer connection, a shared worker, and a service worker are refused by the socket script, a download is denied, and a window is closed. `ActTab::blocked()` reports them as `NAVIGATION`, `OUTSIDE`, the socket's word, `WORKER`, `POPUP`, and `DOWNLOAD`.

`a2m-pack-web-probe` is given an `Observer`, so it can hold an `ObserveTab` or a `WatchTab` and never an `ActTab`.

### The watching tab

A `WatchTab` ([decision 0392](../../decisions/0392-a-page-that-reads-by-a-method-that-may-write-is-learned-from-a-persons-watched-use-and-read-through-an-asserted-request.md), point 1; built in package E2c, [decision 0411](../../decisions/0411-the-watching-tab-records-from-the-browsers-network-events-and-keeps-each-screen-without-its-values.md)) is a tab of a window, opened as the sign-in window is: the browser started afresh, its first tab the engine's notice, which says what is kept. `Observer::watching_tab` sends it once to a `WatchStart`, an address on the surface's origins that does not look like an action, and the engine sends it nowhere after that. It has no `Look`, no input, no `fetch`, and no read from inside the page: its public methods are listed above, and a test of the crate's source holds them to that list.

| What | How |
| --- | --- |
| A request of the page, whatever its method | Not failed: every request in the tab is the person's. Only documents and what a page loads with no body are held, to the tab's origins, as in a sign-in tab; a move to another origin is ended and reported as `NAVIGATION` |
| What is recorded | Every request to one of the tab's origins that a script or a beacon sends, and a document's request that carries a body (a form the person posts), from the browser's network events (`Network.requestWillBeSent`), as a `PageExchange`: its names and kinds as an observing tab keeps them, and its body's media type and, for JSON or a form given whole up to `LONGEST_WATCHED_BODY`, the names and kinds of its fields by the rule of an answer's. The bytes are dropped when the names are read. A request the document left behind unanswered when the next document of the tab loads is closed then: one whose answer's head came is an exchange with no answer fields, one with none is forgotten, and one whose answer came whole and is being read is left to the read, which records it once ([decision 0415](../../decisions/0415-a-request-whose-answer-is-being-read-is-closed-by-the-read-alone.md)) |
| A screen | When the address of the tab's document changes, by a new document or in the same one, and is on its origins, the tab waits until the browser says the document is idle on the network and no recorded request awaits its answer, or `SETTLE_BOUND`, and takes a snapshot, at most `SCREENS_KEPT`. Every element's value is dropped but a password's `MASK`; names are kept as in any snapshot, and the web probe keeps them as types where they are data |
| Workers and frames | Not attached to: their requests are not recorded |
| The service worker | Passed by, so that the page's requests reach the network and are seen | An observing tab's safe requests are a list every caller leaves empty today; package E7c has the drivers fill it from the asserted reads of a published version. Since package E7b ([decision 0394](../../decisions/0394-an-asserted-read-of-a-web-surface-is-a-page-request-matched-by-one-shared-shape-and-shown-at-publish.md), point 3) a safe request is matched by template and body, and the match by prefix is gone.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-pack-web-ui` | `Browsers::new` inside its `Pack::register`; `Browsers::open`, `ActTab` and `Look` for the `ui` and `site_tools` drivers; `Observer`, `ObserveTab::read_from_page`, `ActTab::write_from_page`, `PageWrite`, `PagePath`, and `plain_segment` for `page_http` and the `ui` driver's `goto`; `Visibility::Window`, `export_state`, and `import_state` for its auth providers, once they are built |
| `a2m-pack-web-probe` | `Observer` only: observing tabs, `Look`, `blocked()` |
| `anything2mcp` | Nothing of this crate by name. It registers the web pack, which builds the one `Browsers` and hands its `observer()` to the discoverers |

It calls the scoped supervisor in `a2m-provider` to start and stop browser processes, and `Redact` on every text that leaves it.

## State

| State | Where | Lifetime |
| --- | --- | --- |
| Open browsers, by profile, with tab and run counts | Memory | The engine process |
| A browser's data directory | `profiles/<target>/<environment>/<kind>-<digest>/` in the data directory: the browser's own preferences and caches, no cookie and no page storage | Until the session is revoked. Erasing it is not built |
| Cookies and page storage of its pages | The browser's in-memory context | The browser process |
| Known browser digests | The settings group of the pack that builds the `Browsers`, under `browser_digests` | Read at start, by `BrowserSettings::with_pack_settings` |

It owns no table. A session's state is taken out by `export_state` and sealed and stored by the session keeper, not here.

## Failures

The base returns `BrowserError`. The crates above map it, knowing what the step was. `BrowserError::failure` gives the conversions for errors that mean the same whatever the step, and `None` for the rest:

| Error | Class | Code |
| --- | --- | --- |
| `LaunchFailed(NotInstalled)` | `busy` | `browser_unavailable` |
| `LaunchFailed(Supervisor(_))` | The supervisor's own | For a program the manifest does not declare: `policy`, `effect_not_declared`. Also its limits and its start failures |
| `LaunchFailed(DataDirectory(_))` | The checked path's own | `input`, `path_not_allowed`; `policy`, `effect_not_declared` for a root the manifest does not declare |
| `LaunchFailed(NotOwned(_))`: the pipe ended or carried something else, another protocol version, or an unknown digest | `unknown_variant` | `browser_not_owned` |
| `LaunchFailed(NoAnswer(_))`: the pipe stayed open and nothing answered before the launch deadline | `transient` | `timeout` |
| `LaunchFailed(MemoryShort(_))`: the browsers hold their memory ceiling or more, or one could not be measured; nothing was started (decision 0425) | `busy` | `memory_short`, with the retry time |
| `Crashed` or `Disconnected` with `NotSent` | `transient` | `connection_lost` |
| `Timeout` with `NotSent` | `transient` | `timeout` |
| `LaunchFailed(DataDirectory(_))` for a folder others can read and the engine cannot make private | `input` | `path_not_allowed` |
| `PasswordField` | `policy` | `password_read_refused` |
| `ForeignOrigin` | `policy` | `surface_not_declared`, the code the packs built on this crate already declare |
| `Address::new` refusing a text | `input` | `address_invalid` |
| `PagePath::fill` refusing a value | `input` | `address_value_refused` |

`Crashed`, `Disconnected`, `Timeout`, or `UnexpectedDialog` with `Sent` or `Unknown`, and `RequestFailed`, are never converted here: the driver above decides, and for a commit it is `possible_commit`. `NotFound`, `Ambiguous`, `NotActionable`, `NotLoaded`, `TooLarge`, and `Unexpected` are the driver's to class too: a query that finds nothing is `drift` for a screen model and an answer for a probe.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| A reply reaches the call with its number; an event reaches the subscriber of its session; a deadline ends a call; a malformed or oversized message, or a reply to no request, closes the link and fails everything pending, each saying whether its request was written; a dropped request is never written | Nine tests against a scripted peer on an in-memory stream, with no browser |
| No browser starts with a data directory outside the profile root, or from a program the manifest does not declare | A test with a planted link, through the checked-path constructor; a test with an undeclared program |
| A program that is not a browser, or whose digest is not a known one, is closed and leaves nothing | A test with the test's own program substituted; a test with another digest |
| Two identities never share a process or a data directory | A test that opens two profiles and asks the supervisor |
| A watching tab fails no request for its method, records each with its body's names and no value, and the engine sends nothing of its own in it; a move to an undeclared origin is ended; a snapshot is taken of each screen, with no value | `a_watching_tab_records_every_method_by_structure_and_the_engine_sends_nothing`, in a window on a virtual display: the stand's log holds only what the person did and the one address opened, the save is stored once, a planted secret is in nothing kept, and a body's field named by it is withheld; `a_watching_tab_has_no_method_that_acts` |
| A GET, HEAD, or OPTIONS that carries a header overriding its method is failed in an observing tab, as a request that may write is | `an_observing_tab_lets_through_a_safe_request_only_on_method_origin_template_and_body`: three such reads, none heard by the stand |
| A request whose address's query carries a `_method` entry (`QUERY_METHOD_OVERRIDES`, beside `METHOD_OVERRIDES`; any letter case, percent-decoded, any position, any value or none) is failed, a GET among them | `an_observing_tab_lets_through_a_safe_request_only_on_method_origin_template_and_body`: a GET to `?_method=DELETE`, none heard by the stand; `a_read_whose_query_carries_a_method_override_is_failed` (unit) |
| An observing tab sends no request other than GET, HEAD, OPTIONS, or a declared safe one, from the page and from a worker, and makes no socket | A test in which the page's own script, and a worker, write: the test application stores the write from an acting tab, and its control interface counts none from an observing tab |
| A page cannot move an observing tab, and no `goto` ends on an origin the tab was not given | A test with six pages of the test application that try, by script, a refresh tag, a form with GET and with POST, a beacon, and a move to another origin, and a `goto` that is redirected there: the control interface counts nothing |
| A download writes no file, and a window a page opens fetches nothing, in either kind of tab | A test that clicks a download link and a link that opens a window, and a page that opens one with a gesture: no file in any place a browser saves to, and nothing counted |
| A frame from another site runs, locked in every document it loads | A test whose frame opens a socket and sends a POST in two documents: four refusals reported, nothing counted |
| A service worker woken by an observed page writes nothing, and no frame a page makes gives it an unlocked socket | A test in which an acting tab's page cannot register the worker, a sign-in tab's page does, an acting tab's page wakes it and its POST to its own origin arrives while its socket is refused, and then an observing one wakes it |
| A page, a frame in another process, and a worker send nothing to an origin outside the tab's reach, in either posture; what the tab may reach arrives | `a_tab_sends_nothing_to_an_origin_outside_its_reach_and_what_it_may_reach_arrives` (an image, a script, a style, a `fetch` with GET and POST, and a socket, each counted by the test application) and `a_frame_in_another_process_of_an_acting_tab_sends_nothing_outside_the_tab_s_reach` (a third origin, a second test application, hears nothing) |
| A tab goes to, looks at, and acts on no origin it was not opened for | A test of `goto`, a redirect, a page that moves itself after `goto` returned, `fetch`, and a page with no origin placed in the tab: each is `ForeignOrigin`, nothing is counted, and the form's page still takes input |
| A request marked safe passes only on its method, its origin, its path template segment for segment, and a body whole, of its media type, named once, within 1 MiB, and fitting its closed shape with any `enum`; every other is failed before it leaves the browser ([decision 0394](../../decisions/0394-an-asserted-read-of-a-web-surface-is-a-page-request-matched-by-one-shared-shape-and-shown-at-publish.md)) | `an_observing_tab_lets_through_a_safe_request_only_on_method_origin_template_and_body`, against the stand's `/cases/rows-by-post`, whose control interface shows nothing stored or heard for each of thirteen refusals; the unit tests of `src/safe.rs` for the template, the body, a form, and a body the browser does not give whole |
| No page of either kind of tab can start a shared worker | `a_shared_worker_is_refused_to_a_page_of_either_kind_of_tab`, directly and through a frame |
| No permission is granted and no prompt is left open, in either kind of tab | A test that asks for a location, notifications, and the clipboard with a gesture, and the state of twenty permissions, and opens a file chooser |
| A profile folder is its owner's alone | A test, on Linux and macOS, of a new folder and of one left open |
| A late element is noticed without polling; a wait survives a navigation | A test with an element added by a timer of the page, and a page that moves on while the wait is under way |
| An ambiguous query fails; it never picks the first | A test: four inputs match, and the error says four |
| No password, and no value the redactor knows, leaves the crate in a read, a title, a snapshot, a selector's answer, or a message cut to length | A test with a planted secret, three kinds of password input of which one is in a closed shadow root, five selectors that guess, and the secret laid across the cut |
| A snapshot's address carries no query, and `Loaded.address` keeps it | A test that sends a tab to the form with a query |
| A write through the page is stored once, with the values sent | A test that fills and saves the test application's form, and asks its control interface |
| A read from inside an observing tab's page is one GET to the path made, nothing else; a value that leaves its segment, a redirect, and a page with no origin send nothing further; POST from the same page is failed; an answer over the limit is refused whole | `an_observing_tab_reads_from_inside_its_page_with_get_alone_and_its_guard_still_holds`, `a_read_from_inside_the_page_longer_than_the_limit_is_refused_whole`, against the control interface's record of what it saw |
| A browser killed during a call reports whether the request was sent, and nothing is left | A test that kills the browser while a write is held by the test application: the call says `Sent`, a later call says `NotSent`, the next `open` starts afresh, and no process names the data directory |
| A launch dropped after its program started keeps no place: the next start of the profile succeeds, and the shutdown leaves no subsystem and no process | `a_launch_dropped_after_its_program_started_leaves_nothing_behind` and `a_launch_dropped_after_its_program_started_is_stopped_by_the_shutdown`, with a test hook that holds the first launch after its program started |
| Browsers asked for at once, from several sets, start one after the other and all answer | A test that opens six at once and counts the starts under way |
| With no browser, the failure is `busy`, `browser_unavailable`; a test that needs a browser fails with it and never skips | A test with a program that is not there |
| The launch arguments never hold `--no-sandbox` or a flag that hides automation, and the browser reports itself as automated | A test over the fixed list; a test that asks the page |
| The browser takes no proxy from the environment around it, and no extension runs | `the_browser_takes_no_proxy_from_the_environment_around_it`: every proxy variable set for the browser names a listener that stands for a proxy, a page at a name no resolver knows is asked for, and the listener sees nothing (five connections without the flag); `every_launch_connects_directly_and_runs_no_extension` over the fixed list. Run on Linux. On Windows and macOS the browser reads no such variable, so there the test passes with or without the flag: CI shows only that a browser started with both flags works; that the system's own proxy settings and automatic discovery are refused there rests on what the flag is documented to do |
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
6. Done ([decision 0292](../../decisions/0292-a-web-tab-sends-only-to-its-surface-and-the-destinations-its-tool-declares.md)): every request of a tab a tool runs in held to the tab's reach, `Origins::and_requests_to`, `OUTSIDE`; an acting tab's interception of every request, its locked children, and its socket script. Done in part ([decision 0335](../../decisions/0335-a-scout-carries-the-origins-a-page-tried-and-a-surface-may-refuse-origins-quietly.md)): the sign-in tab's subresources, and `Origins::and_refused_quietly`. Not built: a host name looked up or a connection opened without a request (measured: this headless browser made none). Done: `Observer` and `ObserveTab` with its interception, the worker rule, and the refusal of sockets. Done after the safety audit ([decision 0143](../../decisions/0143-the-browser-engine-after-its-safety-audit.md)): `Origins` on every tab; the lock on a tab's moves; downloads denied, permissions refused, and windows closed; the lock of a frame in another process; the private data directory; the settings reader for digests.
7. Not built, slice 1.6: declared frames; the check of a helper's match against the browser's own accessibility query; recycling by run count and age (recycling past a memory limit is built, [decision 0425](../../decisions/0425-a-browsers-memory-is-measured-from-its-own-list-of-its-processes-and-past-a-ceiling-no-new-browser-work-starts.md); recycling after repeated hangs is built, [decision 0422](../../decisions/0422-a-tab-that-answers-not-even-a-probe-is-closed-and-three-hangs-recycle-its-browser-between-runs.md)); the stable-position check before a click; `Input::Scroll`; one telemetry span per round trip group.
8. Done in part ([decision 0245](../../decisions/0245-a-web-application-is-signed-in-to-through-its-own-sign-in-page-with-a-credential-stored-for-the-person.md)): the in-memory context, `export_state` and `import_state` of cookies. Done ([decision 0254](../../decisions/0254-a-person-signs-in-in-a-window-of-the-engines-own-browser-and-the-web-pack-stops-on-every-declared-sign-of-expiry.md)): `Visibility::Window` with its notice tab, the sign-in tab, the screen a window needs on Linux, `import_state` held to the run's origins, a test screen (`test_support::Screen`), run here on a virtual display. Done (package E2c, decision 0392, point 1): the watching tab, `Observer::watching_tab`, `WatchStart`, `WatchTab`, and its `TrafficCapture`. Not built: page storage in a session's state.
9. Done in part: the page's own GET exchanges in an observing tab, by structure ([decision 0187](../../decisions/0187-the-web-pack-matches-a-screen-by-its-address-and-reads-by-key.md)); a read from inside the page, GET alone, in an observing tab ([decision 0190](../../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md)). A write from inside the page with the page's own token, in an acting tab alone ([decision 0206](../../decisions/0206-the-page-http-mode-writes-by-put-with-the-pages-own-token-in-an-acting-tab.md)). Not built, package E2 and later: platform profiles as data; the stream of every network exchange, of an acting tab, and the `Response` wait; pop-ups and downloads a step declares; uploads; virtualized scrolling; erasing a data directory; a browser installed by `browser.install`.

## Open points

- The Windows handoff of the pipe is proven: the hosted Windows machine started browsers over the two inherited handles once the supervisor connected its ends of the pipes. macOS passed too. On that Windows run the first four browsers, started at the same moment on a cold machine, each took more than the 30 seconds then allowed; hence one start at a time and the longer ceiling. How long one cold start alone takes there has not been measured. The variables passed on Windows are the ones that worked; whether fewer would do is not known.
- The first browser version that takes `--remote-debugging-io-pipes` is not known, so the minimum browser version is not set, and the version a browser reports is not compared with anything.
- The fixed list of known places was written from what the hosted build machines and the common installers use. It has not been collected from a primary source, and per-person installs, which have no fixed path, are not on it.
- The supervisor digests the program before every start. A browser program is about 300 MB, and digesting it is most of a cold start in an unoptimized build. Whether the digest of an unchanged file may be remembered is the supervisor's question.
- The helper computes role and accessible name by the common steps of the naming rules, not all of them. A name it computes differently from the browser makes a field not found, which fails closed; a wrong match is what a fingerprint above is for. Whether the browser's own accessibility query checks every locate is for slice 1.6 and research brief 3.
- The refusal of sockets in an observing tab is a script in the page. A browser-level refusal was looked for twice and not found: the browser's list of blocked addresses does not stop a WebSocket. A shared worker is refused when the page makes it, in either kind of tab; whether one made past that refusal would be attached was not measured.
- A hidden tab's request for a location is held by the browser, unanswered, until the tab is shown, though the permission is denied. A step that waits on such a page ends at its deadline.
- An observing tab knows the `goto`'s own request as the first, since the `goto` began, that the page did not announce. If the browser's announcement of a page's move came after that move's request while a `goto` was under way, the page's request would pass, as a GET to one of the tab's origins. The order has not been seen to differ.
- On Windows the data directory is as private as the folder it is made in. Reading or setting its access list needs the system's own interface, which this crate does not bind.
- An acting step's keys and clicks go to the document that has the focus when they arrive. Between the check and the input the page can have moved, though only within the tab's origins; the stable-position check of slice 1.6 is where this belongs.
- Tabs of one origin in one browser can talk to each other, through a broadcast channel or shared storage. An observed page could ask an acting tab's page to write.
- No session longer than a few seconds has been run. The memory tests (`src/tests/memory.rs`) write each process the browser names, with its kind and figure, and the browser's sum to the test output, each line starting `a2m-memory:`; CI runs them a second time with their output shown, in the step "The memory figures of a browser" (package OS4). Measured on Linux on 2026-11-24, on a four-core machine with 15 GiB, with Chrome for Testing 153 and the test application's form in one tab, by proportional set size: the browser held 317 to 327 MiB (333,000,704 and 342,935,552 bytes, one figure from each memory test) in twelve processes: the browser process 79 to 81 MiB; eight renderers, 9.4 to 57 MiB each and 196 to 205 MiB together; the GPU process 22 to 23 MiB; the network service 23 to 24 MiB; and the storage service 4.4 to 4.7 MiB. A browser with one tab runs eight renderers, so the renderers, not the tab, are most of what it holds. The same tests on CI's runners, in run 45515865670 of 2026-11-25, with one tab of the test application: on macOS, by physical footprint, the browser held 266 to 273 MiB in thirteen processes (the browser process 60 to 61 MiB, nine renderers 11.5 to 26 MiB each, the GPU process 39 to 44 MiB, the network service 11 to 12 MiB, the storage service 5.7 MiB); on Windows, by private commit, 188 to 191 MiB in nine processes (the browser process 45 to 47 MiB, five renderers 14 to 30 MiB each, the GPU process 16 MiB, the network service 10.5 MiB, the storage service 7.8 MiB). Every renderer was measured on all three systems. The three systems measure three different things, and the same browser comes to between 188 and 327 MiB; the limit of 2 GiB is more than six times the largest, and the ceiling of 4 GiB more than twelve, on each. So one pair holds for all three systems, and none is set per system (package OS4). The memory limit and ceiling are drafts until a long run is measured, and hypothesis H-01 needs one too.
- The memory of a browser's processes is measured on macOS and Windows by system calls that are type-checked on Linux and first run in CI; whether a sandboxed renderer can be measured by its owner on each system rests on that run. One that cannot is unmeasured, and the engine then fails closed.
- The three systems' figures are not one measure. Linux's proportional set size counts what is resident and shares each shared page among its processes; macOS's physical footprint counts what is resident or compressed and private; Windows's private commit counts what is promised to the process whether or not it is resident, and a browser commits far more than it touches. `MEMORY_LIMIT` and `MEMORY_CEILING` are one pair of numbers over all three, set from the Linux figure alone, so on Windows the same browser may stand nearer its limit, and be recycled or refused sooner, for no more use. Roadmap package OS4 reads a browser's figures on each system from CI and confirms the pair, or sets one per system by a record ([review of lens 13](../../reviews/other-operating-systems-review-2026-11-24.md), finding 4).
- A declared download is moved from the writing folder by one rename once the browser says it is whole (decision 0419). On Windows a file just written can be held open for a moment by the system's own scan of new files, and a rename refused then deletes the file and stops the step; on Windows and macOS a downloaded file also carries the system's mark of a file from the network, which a program that later opens it may act on. Whether either happens is shown only by the download tests on those two systems' CI, and no run has been read for it yet.
- A renderer held by something other than its own script, such as a loop in layout, is not ended by `Runtime.terminateExecution`; its hung tab is closed and counted, and its process ends only with the browser's recycle between runs. Whether one tab's hung renderer is shared by another tab of the same site in the same browser, which would then hang too, was not measured; the hang count recycles such a browser all the same.
- Whether the browser's sandbox starts on the hosted Linux build machine as it is.
- Fetching and pinning a Chromium build without another project's installer ([research note 09](../../research/09-rust-building-blocks.md)). The `browser.install` action needs it.
- Whether a GET that a page treats as an action can be recognized at this level, or only by the probe's rules above. [Section 402](../../spec/4-building-tools/402-the-scout-and-who-does-what.md) leaves it to Phase 4.
- A write from inside the page is sent again by the browser itself when its connection closes with no answer (decision 0206). No option of the page's `fetch` stops it. A crate above takes every answer to a write as the answer to the last sending, and builds only a write that sending twice leaves as once.
- Whether observing and acting tabs in one browser are safe enough together, or an observing scout should use its own browser on a copy of the profile. Copying a profile is what decision 0032 avoids. The point above is one reason to doubt it.
