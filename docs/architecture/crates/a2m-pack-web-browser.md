# `a2m-pack-web-browser`

The base of the web pack: a browser engine that speaks the DevTools protocol to Chrome, Edge, or Chromium, launched with a data directory the engine owns. It launches and recycles browsers, opens tabs, locates elements by role and name, acts on them, waits on events, observes the network, and reports crashes. It is not a driver and registers nothing; `a2m-pack-web-ui` and `a2m-pack-web-probe` are built on it.

## Facts

| | |
| --- | --- |
| Side | Pack: web |
| Layer | 9, Pack bases |
| May depend on | Layers 1 to 5 |
| Depends on | `a2m-core` for `Failure`, `Secret`, `Redact`, `Digest`; `a2m-telemetry` for spans; `a2m-provider` for the pack context: the scoped supervisor, checked paths, the redactor, telemetry. Outside: `tokio`, `serde_json`, and the base library chosen by slice 0.4, an outside crate, to be verified |
| Built in | Slice 0.4 chooses the base. Slice 1.1 builds launch, one tab, one locate, one read. Slice 1.6 the rest of locating and waiting. Package 2.3 the visible window. Package 4.2 in-page requests. Package 8.5 the hard cases |
| Carries out | Spec sections [302](../../spec/3-packs/302-the-web-pack-browser-engine.md), [215](../../spec/2-kernel/215-sessions-and-the-session-keeper.md) (profiles and tabs), [204](../../spec/2-kernel/204-supervising-subsystems.md) (for the browser), [225](../../spec/2-kernel/225-performance.md) (the browser budgets); decisions [0004](../../decisions/0004-rust-browser-engine-on-the-devtools-protocol.md), [0031](../../decisions/0031-the-browser-always-runs-with-a-profile-the-engine-owns.md), [0032](../../decisions/0032-one-live-session-per-identity-shared-by-tabs-with-a-single-renewer.md) |

## Owns

- Launching a browser program, through the supervisor the pack context scopes to the programs its manifest declares, with an engine-owned data directory, one per identity, given as a checked path.
- One browser process per profile, shared by many tabs. Recycling by run count, age, and memory.
- Tabs with a fixed environment: viewport, locale, time zone, color scheme, reduced motion.
- Resolving a query to exactly one element, through the accessibility tree and an injected helper, across open shadow roots and declared frames.
- Actionability checks and input, batched into one in-page call where it can.
- Waits that end on a protocol event or an in-page observer, with a deadline. No polling.
- Building a snapshot from the accessibility tree, in the form of [steps, snapshots, and diffs](../../reference/steps-snapshots-and-diffs.md#snapshots), so that a run's evidence and discovery's findings use one builder.
- Platform profiles as data: their schema, the built-in set, and evaluating a profile's detection rules and readiness check against a page. Both crates above need them, and this is the only crate both may use. Applying a profile to locating and input is `a2m-pack-web-ui`'s; choosing one for a surface is `a2m-pack-web-probe`'s.
- Network observation, with credentials removed by structure before anything leaves the crate. Requests sent from inside the page.
- The posture of each tab: one that may act, or one that may only observe (below).
- Detecting a crash, a disconnect, a dialog, a pop-up, and a download, and saying for each act whether its input reached the page.

It must never: start a process except through its scoped supervisor; connect to any browser process it did not start; resolve a path itself, or open a data directory it was not handed as a checked path; read or copy another program's cookie store; hide that it is automation, alter its fingerprint, or answer a challenge; know about screen models, widget drivers, endpoints, or modes; decide whether a failure means a write may have landed; retry.

## Interface

A sketch. The base library is behind this interface and does not appear in it: no type from it is public, so that slice 0.4 can choose, and a later decision can change, without touching the crates above (hypotheses H-01 to H-03).

```rust
/// One per engine process. Built by the product crate and handed to both web pack crates,
/// so that a profile is never opened by two browsers.
pub struct Browsers { /* launched browsers by profile; the supervisor's handles */ }
impl Browsers {
    pub fn new(ctx: &PackContext, settings: BrowserSettings) -> Result<Self, Failure>;
    /// What is installed and whether its digest is known. Starts nothing.
    pub fn availability(&self) -> Availability;
    /// The profile's browser, launching it on first use; one launch per profile at a time.
    pub async fn open(&self, profile: &ProfileKey, show: Visibility) -> Result<Browser, Failure>;
    /// A handle that can open observing and watching tabs and nothing else. The product crate
    /// gives `a2m-pack-web-probe` this, never `Browsers`.
    pub fn observer(&self) -> Observer;
    pub fn list(&self) -> Vec<OpenBrowser>;               // for doctor.run and the supervisor listing
    /// Closes the browser; with `Erase`, deletes the data directory. Reports cleanup, complete or not.
    pub async fn close(&self, profile: &ProfileKey, how: Close) -> Cleanup;
}

pub struct ProfileKey { /* target, environment, identity: maps to profiles/<identity>/ */ }
pub enum Visibility { Background, Window { purpose: String } }   // a sign-in window shows its purpose

pub struct Browser { /* cheap to clone; one process */ }
impl Browser {
    pub async fn acting_tab(&self, env: &PageEnvironment) -> Result<ActTab, Failure>;
    pub fn health(&self) -> BrowserHealth;                 // idle, busy, exited, unreachable
    pub fn events(&self) -> BrowserEvents;                 // crash, disconnect, pop-up, download
    /// Cookies and storage, sealed for the vault. Called only by the web pack's auth providers.
    pub async fn export_state(&self) -> Result<Secret, Failure>;
    pub async fn import_state(&self, state: &Secret) -> Result<(), Failure>;
}

/// Everything that only looks. Both tab kinds have it.
pub trait Look {
    async fn goto(&self, address: &Address, deadline: Deadline) -> Result<Loaded, BrowserError>;
    async fn locate(&self, q: &Query, deadline: Deadline) -> Result<Element, BrowserError>;
    async fn read(&self, el: &Element, what: ReadWhat) -> Result<RawValue, BrowserError>;
    async fn wait(&self, until: &Primitive, deadline: Deadline) -> Result<Waited, BrowserError>;
    async fn tree(&self, limits: &TreeLimits) -> Result<AxTree, BrowserError>;
    /// The snapshot of the reference document, without screen matches: only the crate that
    /// knows screen models can add those. Restricted and password values are masked here.
    async fn snapshot(&self, limits: &SnapshotLimits) -> Result<Snapshot, BrowserError>;
    fn network(&self, filter: &ExchangeFilter) -> Exchanges;  // a stream of redacted exchanges
}

pub struct Observer { /* a Browsers that can only look */ }
impl Observer {
    pub async fn observing_tab(&self, profile: &ProfileKey, env: &PageEnvironment, allow: &SafeRequests)
        -> Result<ObserveTab, Failure>;
    /// A visible window in which a person uses the system while the tab only listens. No
    /// interception: what the person saves is the person's act. Opened only for a capture the
    /// person started ([section 213](../../spec/2-kernel/213-effects-hazards-and-consent.md)).
    pub async fn watching_tab(&self, profile: &ProfileKey, env: &PageEnvironment, purpose: &str)
        -> Result<WatchTab, Failure>;
    pub fn profiles(&self) -> &Profiles;                  // the platform profiles, as data
}

pub struct ObserveTab; impl Look for ObserveTab { /* ... */ }
pub struct WatchTab;   impl Look for WatchTab { /* ... */ }
pub struct ActTab;     impl Look for ActTab { /* ... */ }
impl ActTab {
    async fn check(&self, el: &Element) -> Result<Actionable, BrowserError>;
    async fn input(&self, el: &Element, how: &Input) -> Result<Dispatch, BrowserError>;
    async fn fetch(&self, req: &PageRequest, deadline: Deadline) -> Result<PageResponse, BrowserError>;
    async fn answer_dialog(&self, how: DialogAnswer) -> Result<Dispatch, BrowserError>;
    async fn attach_file(&self, el: &Element, file: &StagedFile) -> Result<Dispatch, BrowserError>;
}

pub enum By { RoleName { role: String, name: String }, Label(String), Placeholder(String),
              Text { text: String, contains: bool }, TestId(String), Title(String), Css(String) }
pub struct Query { by: By, within: Option<Box<Query>>, frame: FramePath, shadow: ShadowScope }

/// The waits the base can end on an event. Conditions of the file formats are built from these.
pub enum Primitive { Visible(Query), Hidden(Query), Enabled(Query), Text(Query, TextMatch),
                     Url(UrlPattern), Response(ExchangeFilter), Script(HelperCheck),
                     All(Vec<Primitive>), Any(Vec<Primitive>) }

pub enum Input { Fill(String), Type(String), NativeSet(String), Press(Key), Click, Check(bool),
                 SelectByValue(String), Scroll(ScrollTo) }

/// Whether an act reached the page. The crate above decides what that means for a write.
pub enum Dispatch { Sent, NotSent, Unknown }

pub enum BrowserError {
    NotFound, Ambiguous { count: usize }, NotActionable(Reason), Timeout,
    Crashed { dispatch: Dispatch }, Disconnected { dispatch: Dispatch },
    UnexpectedDialog, UnexpectedPopup, Blocked { request: RequestShape }, LaunchFailed(LaunchReason),
}
```

From the kernel: `a2m-provider` hands a pack a `PackContext` at registration, holding a supervisor scoped to what the pack's manifest and the published plan declare, the pack's roots in the data directory as checked paths (here, the profile root), the redactor, and telemetry. The product crate builds `Browsers` from the web pack's context. A path can only be made by `a2m-provider`'s check against the allowed roots; this crate never resolves one.

Contracts that the signatures do not show:

- **The base library stays inside.** A private trait inside the crate is the only place that speaks the protocol. The conformance suite runs against the public interface above, so two bases can be compared by the same tests.
- **The DevTools connection is a pipe, and a foreign link.** The browser is started with its protocol on an inherited pipe, not a port, as section 204 asks. The protocol is fixed by someone else, so there is no handshake of ours in it: the supervisor's ownership of the process rests on the program's digest, the version the browser reports over the pipe, and the owner mark on the launch. A browser whose reported version or digest does not match what was launched is closed. A base that cannot use a pipe is marked down in the spike.
- **A data directory is a checked path.** It is made by `a2m-provider`'s check against the profile root, links followed, before this crate sees it. This crate builds no path from a string.
- **An element is valid for one page generation.** A navigation or a re-render that removes the node makes it stale, and using it is `NotFound`, never a silent re-query.
- **The helper runs in an isolated world.** The injected script that locates, checks, and observes cannot be read or changed by the page's own scripts.
- **Exchanges leave the crate redacted.** Cookie, authorization, and set-cookie headers are removed by name; every value the redactor knows is replaced. Bodies are kept only when the caller's filter asks for them.
- **A password input's value is never read.** `read` refuses it, whatever the caller asks.
- **Recycling waits.** A browser due to be recycled takes no new tab and is closed when its last tab closes.

### Two postures

An `ObserveTab` cannot act, because it has no method that acts. That is the first lock. The second is in the browser itself, so that the page's own scripts cannot write either:

| What could change the system | In an observing tab |
| --- | --- |
| A request whose method is not GET, HEAD, or OPTIONS | Intercepted and failed before it leaves the browser, unless it matches the tab's `SafeRequests`, which only a person sets ([section 401](../../spec/4-building-tools/401-overlaying-tools-on-an-existing-system.md)). Each one is reported |
| A request from a worker the tab starts | The same interception is attached to every worker before it runs; service workers are bypassed |
| A WebSocket | Refused at creation, and reported as a limitation |
| A beacon, a form submission, a download | Failed by the same interception, or refused |
| Navigation | Only through `goto`, to an address the caller passes |

`a2m-pack-web-probe` is given an `Observer`, so it can hold an `ObserveTab` or a `WatchTab` and never an `ActTab`. Neither has an input method. A `WatchTab` carries no interception because the person in the window is entitled to save; nothing the engine sends goes through it.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-pack-web-ui` | `Browsers::open`, `ActTab` for the `ui`, `page_http`, and `site_tools` drivers; `Visibility::Window`, `export_state`, and `import_state` for its auth providers |
| `a2m-pack-web-probe` | `Observer` only: observing and watching tabs, `Look`, the platform profiles |
| `anything2mcp` | `Browsers::new`, once; it hands the `Browsers` to `a2m-pack-web-ui` and its `observer()` to `a2m-pack-web-probe` |

It calls the supervisor in `a2m-provider` to start, list, and stop browser processes, `Redact` on every exchange and error text, and `a2m-telemetry` for one span per protocol round trip group.

## State

| State | Where | Lifetime |
| --- | --- | --- |
| Open browsers, by profile, with run count, age, and memory | Memory | The engine process |
| A browser's data directory | `profiles/<identity>/` in the data directory | Until the session is revoked; deleted on `Close::Erase` |
| Known browser digests | Settings and the managed component list | Read at start |
| Built-in platform profiles | Data compiled into the crate, versioned with it | The binary |

It owns no table. A session's saved state is sealed by `export_state` and stored by the session keeper, not here.

## Failures

The base returns `BrowserError`. The crates above map it, knowing what the step was. Its own conversions, for errors that mean the same whatever the step:

| Error | Class | Code |
| --- | --- | --- |
| `LaunchFailed`, browser not installed | `busy` | `browser_unavailable` |
| `LaunchFailed`, program not one the scoped supervisor allows | `policy` | `path_not_allowed` |
| `Crashed` or `Disconnected` with `NotSent` | `transient` | `connection_lost` |
| `Timeout` | `transient` | `timeout` |
| `Blocked` in an observing tab | Not a failure: a reported event |

`Crashed` or `Disconnected` with `Sent` or `Unknown` is never converted here: the driver above decides, and for a commit it is `possible_commit`.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| No browser starts with a data directory outside the profile root, or from a program the manifest does not declare | A test with a planted link, through the checked-path constructor; a test with an undeclared program |
| A browser whose reported version or digest differs from the launch is closed | A test with a substituted program |
| Two identities never share a process or a profile | Conformance group "Isolation" |
| `a2m-pack-web-probe` never names `ActTab` or `Browsers` | A check in CI over the crate's source, and the type it is constructed with |
| An observing tab sends no request other than GET, HEAD, OPTIONS, or a declared safe one, including from page scripts and workers | A test-application page that posts on load, from a worker, and over a WebSocket; the control interface counts no write |
| A late element is noticed without polling | Conformance group "Waiting"; the spike's measurement for H-03 |
| An ambiguous query fails; it never picks the first | Conformance group "Locating" |
| No cookie, token, or password leaves the crate in an exchange or an error | The planted-secret test of package 2.1, run against captured exchanges |
| A crash during an act reports whether input was sent | A fault test that kills the browser after dispatch |
| The browser budgets of section 225 hold | Benchmarks: a warm tab under 100 ms; a cold start under 1 s |

## Build order

1. Slice 0.4, in `spikes/`, not here: each candidate base against launching, locating, and waiting. A decision record.
2. Slice 1.1: `Browsers::new`, `open`, one `ActTab`, `locate` by role and name, `read`. Pipe connection, ownership by digest, version, and owner mark.
3. Slice 1.6: every `By`, frames and shadow scopes, every `Primitive`, `tree` for snapshots, recycling.
4. Package 2.3: `Visibility::Window`, `export_state`, `import_state`.
5. Slice 3.3: `check` and `input`, with `Dispatch` reported.
6. Package E2: `Observer`, `ObserveTab` with its interception and the worker and WebSocket rules, `WatchTab`. Profiles' detection rules.
7. Package 4.2: `fetch` from inside the page.
8. Package 8.5: pop-ups, downloads, uploads, virtualized scrolling.

## Open points

- The base library: chromiumoxide, Rustwright's core, or a new protocol client. Slice 0.4; whether it supports a pipe connection is one of its criteria.
- Fetching and pinning a Chromium build without another project's installer ([research note 09](../../research/09-rust-building-blocks.md)). The `browser.install` action needs it.
- Whether a GET that a page treats as an action can be recognized at this level, or only by the probe's rules above. [Section 402](../../spec/4-building-tools/402-the-scout-and-who-does-what.md) leaves it to Phase 4.
- Whether observing and acting tabs in one browser are safe enough together, or an observing scout should use its own browser on a copy of the profile. Copying a profile is what decision 0032 avoids.
- How to measure memory per browser on three operating systems for the recycling rule.
