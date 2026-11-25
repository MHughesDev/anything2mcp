# `a2m-pack-web-probe`

The web pack's discoverers and its describer: the capability probe, sign-in detection, snapshots, traffic capture, endpoint inference with two-tier path templates, and what a site declares for agents. It looks at a web application through an engine-owned browser and returns findings. It cannot write, and the way it is built makes sure of that.

Built so far ([decision 0114](../../decisions/0114-the-web-probe-as-built-three-discoverers-over-an-observing-tab.md)): the probe, snapshots, and capture with endpoint inference, over an observing tab. The parts not built are named under "Build order", and a table or sketch below that describes one says so.

## Facts

| | |
| --- | --- |
| Side | Pack: web |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5, and `a2m-pack-web-browser` |
| Depends on | `a2m-core` (`Finding`, `Subject`, `Coverage`, `EvidenceLevel`), `a2m-provider` (`Discoverer`, `Describer`, `SandboxPort`, `PackContext`, registration), `a2m-telemetry` (the clock), `a2m-pack-web-browser` (the cell of browsers, of which only the `Observer` is kept). Outside: `serde_json`. Not yet `a2m-auth`: a discovery's scope carries no session |
| Roadmap packages | Package E2, after E1 gives the discoverer extension point and the findings store, and after 2.6 gives sign-in. `auth.detect` arrives earlier, in package 2.6, as the first discoverer |
| Carries out | Spec sections [301](../../spec/3-packs/301-the-web-pack.md) (the probe), [215](../../spec/2-kernel/215-sessions-and-the-session-keeper.md) (auth discovery), [401](../../spec/4-building-tools/401-overlaying-tools-on-an-existing-system.md) (capture, mining, read-only exploration), [218](../../spec/2-kernel/218-discovery-and-findings.md), [401](../../spec/4-building-tools/401-overlaying-tools-on-an-existing-system.md) (scenarios, templating), [402](../../spec/4-building-tools/402-the-scout-and-who-does-what.md) (stages 1 and 2 for web surfaces) |

## Owns

- Saying what a web surface is: platform, framework, support tier, sign-in method, and candidate surfaces it plainly offers.
- Snapshots as findings: screens, fields, and actions seen, each with its subject and coverage.
- Capture: exchanges seen while a scenario runs or a person uses the system, kept as shapes, attributed to scenario steps.
- Endpoint inference: grouping exchanges into endpoints, templating paths in two tiers, inferring request and response shapes and dependencies between requests.
- Reading what a site declares for agents, including tools a page registers, as untrusted findings.
- The describer: showing an agent, in compact text, what was seen.

It must never: hold an acting tab; press, type, or submit; send a request other than a navigation to an address it was given or found as a plain link; enter a surface the target did not declare; keep a value where a shape will do, unless a person asked and policy allows it; claim `discovered` for anything that did not answer; write a draft file, which is the drafter's work.

## What it registers

It is a provider of its own, named `web_probe`, because a pack is handed its context only while it registers and the two crates of the web pack may not name each other.

| Registration | Content |
| --- | --- |
| Modes, drivers, surface kinds, file kinds, lowerings, lint rules, auth providers | None. The surface kind `web` is registered by `a2m-pack-web-ui` |
| Finding kinds | Built: `surface`, `sign_in_method`, `screen`, `exchange`, each a checked name. It produces `endpoint` findings and does not own that kind: its manifest reads it. Not built: `declared_tool`. `field` and `action` are not kinds: one `screen` finding holds its fields and actions |
| Discoverers | Built: `web_probe`, `web_snapshot`, `web_capture`, `web_auth`, and `web_use`, the one a person drives (`driven_by = "person"`, bound 1800 s). Not built: `web_endpoints`, `web_declared` |
| Describer | `web_describer`, for each discoverer |
| Drafting rules | None of its own. Its findings are drafted by the rules of the crates that own the file kinds: screens by `a2m-pack-web-ui`, which drafts a read of each table a snapshot names, and endpoints by `a2m-pack-http` |
| Effects | The root `profiles` for writing, the network of the target's surfaces, and a requirement that lists every place of the base's `KNOWN_PLACES`; a test holds the two lists equal |
| Settings | `max_pages`, the page budget of one discovery, 20 unless set |

| Discoverer | Action that runs it | Reads | Finding kinds | Level |
| --- | --- | --- | --- | --- |
| `web_probe`, built | `target.probe` | One page load: the status, where the tab ended, whether a password input shows, the roles of the snapshot, and whether the document is JSON. The kind of interface is named by `INTERFACE_RULES`, which are data. Platform profiles are not built, so the platform is `unknown_platform` and the tier `unsupported` | Surface; sign-in method when the page asks for one | The surface `discovered`; the sign-in method `inferred` |
| `web_auth` | `auth.detect` | One page load of the start page, by structure and where it ended, never a field's value, and nothing sent: a password field (`password_form`), a move to another origin, with that origin (`other_origin`), status 401 (`challenge`), a field whose `autocomplete` is `one-time-code` (`code_step`). The finding's `sign_in` is one of `SIGN_IN_SIGNS`; `second_factor` is `asked` when a code field shows and `unknown` otherwise, never guessed (decision 0321). A page whose address reads as a sign-in (`SIGN_IN_WORDS`) and shows none of these stops as `unknown_variant`/`sign_in_unrecognized`; an open page gives no finding. Not built: the challenge's scheme, a request for a client certificate, the identity provider's discovery document, and what backend calls carry | Sign-in method | `inferred`, with the observations as evidence |
| `web_snapshot`, built | `screen.snapshot` | The accessibility tree, through the base's snapshot builder, of the start page, of each page the focus names, and of the pages plain links lead to, as deep as the bounds allow | Screen, with its elements | `discovered`; coverage `partial`, limitation `single_variant` |
| `web_use`, built | A capture job, begun by its person after `capture.start` | A watching tab, in a window, in the profile `watching` of the target and environment, opened once on the surface's own address and sent nowhere after that; read only through `take_capture`, and through the snapshots the tab takes of each screen, each time it has taken one and once more at the end. Every request the pages send to the surface's origin, every method, by structure, with its body's media type and the names and kinds of its fields. A request is put to the screen the person was on when it was taken, which each finding says. Requests of one method to one path from one page are one `exchange` finding with `seen` and `shapes`, and the body as `body`; reads by key are matched as `web_capture` matches them. Each screen as `web_snapshot` gives one, with `seen_by: person` ([decision 0442](../../decisions/0442-a-persons-use-is-watched-by-web-use-and-what-it-saw-twice-is-drafted-held.md)) | Exchange; screen; endpoint | An exchange `discovered`; an endpoint whose path holds a parameter `inferred` |
| `web_capture`, built in part | `scout.run` and scenario runs | The request for each document the tab is sent to, with the shape of a JSON answer; each request a page tried that the tab refused; each request a page's own script made with GET to the surface's origin, as the observing tab records it: method, path, the query's names, status, the names and kinds of a JSON answer's fields to one level, the page that made it (`made_by`), and the names of the headers that carried a credential (`credential_headers`), never a value ([decision 0187](../../decisions/0187-the-web-pack-matches-a-screen-by-its-address-and-reads-by-key.md)); and the names of the headers the page's script set (`set_headers`) and whether an entry of its query was left out (`query_left_out`) ([decision 0195](../../decisions/0195-a-read-never-runs-where-the-page-may-write-and-a-page-keeps-only-its-structure.md)). The wait for one page's own requests is at most 10 s; requests still awaited then, and requests past the 256 a tab keeps, are left out and said to be in the page's limitations. Requests a page made by itself, of one method and origin, one status and one JSON shape, whose paths differ in one segment and whose pages differ in one segment holding the same value, are one `exchange` finding with the placeholder in `path` and `made_by` and the requests in `paths`; theirs are not given apart ([decision 0190](../../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md)) | Exchange; endpoint | An answered exchange `discovered`; a refused one `inferred`. Endpoints as below |
| `web_endpoints`, not built apart | `endpoints.infer` | Stored exchange findings, not the page. Until the sandbox port hands over stored findings, the inference runs inside `web_capture` | Endpoint, with its template, shapes, and dependencies | The endpoint `discovered` if it answered; its template `inferred` |
| `web_declared`, not built | `discover.run`, the scout | A site's declarations for agents; tools a page registers | Declared tool, declared capability | `inferred`, source `published_description`, marked untrusted |

The `endpoint` finding kind is owned by `a2m-pack-http`. This crate declares that it reads that kind, and widens what is found: the first seven names are the owner's (`format`, `method`, `path`, `status`, `content_type`, `allow`, `challenges`), and `answered`, `statuses`, `template`, `query`, `response`, and `seen` are added. If the HTTP pack is not enabled the registry says that the kind's owner is missing, and this pack is registered all the same. Packs share the shape as data, never code: this crate does not know the endpoint file format.

### What a finding keeps

A value is replaced by its type everywhere: `empty`, `integer`, `decimal`, `boolean`, `date`, `date_time`, `uuid`, `hex`, or `text`.

| Finding | Kept | Kept as a type only |
| --- | --- | --- |
| `screen` | `refused_origins`, when the page tried to send to origins outside the surface's own: each origin once, as the tab reported it, with no path and no query ([decision 0335](../../decisions/0335-a-scout-carries-the-origins-a-page-tried-and-a-surface-may-refuse-origins-quietly.md)). The title; each element's role, depth, states, and heading level; the accessible name of a role in `NAMED_ROLES` (a heading, a control, a column header) and of a link outside a record | Every value of a control; the name of any other role, such as a cell or a line of text; the name of a link inside a row or a list item. A password is `password` |
| `exchange` | The method, the path by structure, the status, the plain names of the query's parameters, the names of a JSON answer's fields where they are structure | The values of the query and of the answer; a segment of a path that is not a plain word, kept as `{value}`; an entry of a query with no `=`, or whose name is not a plain name, counted and not named; the names of an object that read as a dictionary's keys (more than 64, or one that is not a plain name), withheld with a limitation ([decision 0195](../../decisions/0195-a-read-never-runs-where-the-page-may-write-and-a-page-keeps-only-its-structure.md)) |
| `endpoint` | The method, the path template, the statuses, the names and types of query parameters and answer fields, how many exchanges it was made from | For each parameter, how many different values were seen, compared by digest; the values are not kept |

Everything found passes the redactor as one text, names included, and so does the path of a finding's subject. Every path a finding keeps is by structure, and a description list keeps the name of a term and of a definition only as a pair, a term that labels a definition of its name; a list used as data keeps types. A matched screen or request keeps the limitations of every page or request it was matched from ([decision 0195](../../decisions/0195-a-read-never-runs-where-the-page-may-write-and-a-page-keeps-only-its-structure.md)). Every finding has coverage `partial` and at least one limitation.

## Discovery cannot write

Rule one of [section 218](../../spec/2-kernel/218-discovery-and-findings.md) is held in four places. Each would hold without the others.

| Where | How |
| --- | --- |
| The type | This crate keeps an `Observer` from the base and nothing else of the cell it is given. An `Observer` opens observing tabs only; one has no input method, and there is no way to obtain an acting tab from it. A check in CI is to refuse the names of the acting types in this crate's source; it must match whole words, because the crate names the cell `SharedBrowsers`. The check is not written yet |
| The browser | An observing tab fails every request whose method is not GET, HEAD, or OPTIONS before it leaves the browser, from the page, its workers, and its frames. This crate marks no request safe. WebSockets are refused at creation. A refused request becomes an `exchange` finding that says the system never answered; anything else refused becomes a limitation on the finding of its page |
| Navigation | A link is followed only when it is a plain link to the surface's own origin, its name is shared with no other link on the page, and neither its address nor its name holds a word of `ACTION_WORDS` (sign-out, delete, remove, approve, confirm, send, pay, and the like). A link not followed is counted in the report, and a link refused as an action is a limitation on the screen. A focus can name a path of the surface and nothing else. Every address the tab is sent to passes the same word check, whatever named it, the surface, a focus, or a link: one that looks like an action is never visited, and stops the discovery with `policy`, `address_looks_like_an_action` ([decision 0144](../../decisions/0144-the-web-drivers-and-discoverers-after-their-safety-audit.md)). A redirect is followed by the browser before this crate sees it: one that ends at such an address is not read, and stops the discovery the same way, after the visit. A page that redirects to an address already looked at, or waiting to be, is that page: it is looked at once, under the address it ended at, and a link to that address is not followed again; and screen matching takes one address once, so one page seen twice is never two pages of a screen at `/{key}` (package 5.6b, [decision 0445](../../decisions/0445-the-first-hours-faults-a-list-on-the-command-line-a-key-off-a-list-and-a-project-held-to-one-store.md)). The tab is opened for the surface's origin, and the browser holds it there: a move the page starts is ended before it leaves the browser, and a redirect to another origin is ended too, which the probe reports as a sign-in by redirect. What "looks like an action" means is settled in Phase 4 |
| The kernel | Discovery runs as a job on a runner, inside the engine's sandbox. A discoverer is given only what `SandboxPort` offers: no driver, no ledger token, no way to reach `Driver::act`. This crate has no driver, so it sends no step through the port at all |

**Capture of a person's use is not exploration.** When a person starts a capture, they use the system in a visible window through a watching tab, and may save. That save is the person's act; the discoverer only listens. The scout never opens a watching tab, and a capture can only be started and stopped by the person whose use it records ([section 213](../../spec/2-kernel/213-effects-hazards-and-consent.md)).

## Endpoint inference

| Step | Rule | Level |
| --- | --- | --- |
| Group, built | By method, origin, and path template. Only documents the tab was sent to and requests the tab refused are seen, so there are no asset requests to leave out yet | The group `discovered` if it answered, `inferred` if every request was refused |
| Template, tier one, built | A segment becomes a parameter only when it is an identifier by format: a number of at least four digits (`LEAST_DIGITS`, a starting value, not measured), a universally unique identifier, a long hexadecimal string, a date. A shorter number stays literal and the endpoint carries a limitation that says so ([decision 0341](../../decisions/0341-endpoint-inference-as-built-the-counts-the-short-number-the-sibling-words-and-the-map.md)) How many different values were seen is kept, by digest; the values are not | The template `inferred`; the endpoint keeps its level |
| Template, tier two, built | A segment that varied between otherwise identical requests: the same method and origin, one differing segment, and JSON answers whose tops hold the same names. Only when the focus sets `tier_two`. Two words are not joined when the capture elsewhere uses each as a literal at that place (the same origin and prefix, a path of another length or another method); the parameter's `values_seen` says how many values it was made from | `inferred`, limitation `sampled`, marked as resting on variance alone |
| Shapes, built for answers and, from a capture, for bodies | Names and types from JSON answers, and from the bodies a watching tab recorded (`request`), merged across exchanges; a field seen in some answers and not others is optional with limitation `sampled`; a field seen in every one of at least two objects carries `seen_in_all` with that count and is never `required`, because presence in every sample is no contract. An object whose keys are all identifiers by format is one `map`, whatever its size, with its keys unnamed and its values' shape as `items`. An observing tab sees no request body | `inferred` |
| Dependencies, not built | A value in a request that equals a value in an earlier response of the same capture: a token or an identifier the request depends on. The method is the digest comparison and nothing else: an equal digest of one value in a request and in an earlier answer is the only evidence taken, no value is stored, and no model is used (rule 1) | `inferred` |
| Binding to an action, not built | The scenario step during which the request was sent. A request seen in two steps is ambiguous and is not bound | As the scenario |
| Front-end logic, not built | A request whose body holds a field no input explains, or a header that changes per request with no source found, is marked as possibly computed or signed in the browser. Its endpoint is drafted for `page_http` only | `inferred` |

A derived finding is never stronger than its weakest parent: one tier-two segment makes the whole endpoint `inferred`.

**It drafts nothing by name.** This crate registers discoverers and no drafting rule: what it keeps of a page's requests and snapshots is a finding, never a file of a target. The rule that a column, a value, or a response value named as a secret is never drafted as an output is the drafter's, in [`a2m-pack-web-ui`](a2m-pack-web-ui.md) ([decision 0302](../../decisions/0302-a-field-named-as-a-secret-is-never-drafted-as-an-input-or-an-output.md)).

## Interface

As built.

```rust
/// The pack, over the cell of browsers the product crate made and also gives to the web pack's drivers.
pub fn pack(shared: SharedBrowsers) -> WebProbePack;
/// The one call the product crate makes: `add_to(&mut builder, shared.clone())`.
pub fn add_to(builder: &mut RegistryBuilder, shared: SharedBrowsers);
impl Pack for WebProbePack { /* registers web_probe, web_snapshot, web_capture, web_auth, each with the describer,
                                and the capture source of `points.traffic_capture = ["web"]` (decision 0391) */ }

pub const PACK: &str = "web_probe";            // the provider's name
pub const SURFACE_KIND: &str = "web";
pub const WEB_PROBE: &str = "web_probe";       // and WEB_SNAPSHOT, WEB_CAPTURE

/// The names a discovery's focus may hold.
pub const FOCUS_PATHS: &str = "paths";            // a list of paths of the surface, each starting with one `/`
pub const FOCUS_READY_TEXT: &str = "ready_text";  // a text that must be visible on a focused page before it is read
pub const FOCUS_TIER_TWO: &str = "tier_two";      // true lets inference make a parameter by variance

/// Rules as data.
pub const INTERFACE_RULES: &[InterfaceRule];   // which kind of interface a start page shows; EMPTY_INTERFACE when none holds
pub const ACTION_WORDS: &[&str];               // what makes a link look like an action
pub fn looks_like_an_action(text: &str) -> bool;
pub const NAMED_ROLES: &[&str];                // roles whose name is structure; also RECORD_ROLES, FIELD_ROLES
pub const DISCOVERY_ACCOUNT: &str = "discovery";

/// Shapes and inference, pure.
pub fn type_of_text(text: &str) -> &'static str;
pub struct Shape;                              // Shape::of(&json), merged, has_optional, cut, to_json
pub struct Exchange { pub method, pub origin, pub path, pub query, pub status, pub answer: Answer,
                      pub made_by: Option<String>, pub credential_headers: Vec<String>,
                      pub set_headers: Vec<String>, pub query_left_out: bool }
pub enum Answer { Refused, Json(Shape), Document }
pub struct Endpoint { pub method, pub origin, pub segments: Vec<Segment>, pub statuses, pub query, pub response, pub seen, pub sources }
pub enum Segment { Literal(String), Parameter { types, tier: Tier, literals } }
pub fn infer_endpoints(exchanges: &[Exchange], tier_two: bool) -> Vec<Endpoint>;
pub struct MatchedRequests { pub pattern: String, pub page: String, pub members: Vec<usize> }
pub fn matched_requests(exchanges: &[Exchange]) -> Vec<MatchedRequests>;   // request matching (decision 0190)

pub mod codes { /* this crate's failure codes, and ALL with the class of each */ }
```

The three discoverers and the describer are private types reached through the registry. `Capture`, with scenario steps, is not built.

What the kernel gives this crate, as built:

- `DiscoveryScope` carries the surface, the bounds (seconds, bytes, depth), a deadline, the redactor, and the focus. It carries no session and no identity, so a discovery looks as nobody, in the engine's own profile `discovery` of the target and environment. A page that asks for a sign-in is a finding of `web_probe`, and stops the other two with `needs_login`.
- It carries no clock, no fixed page environment, and no rate limit. The time of a finding is taken from telemetry's clock at registration; the page environment is the base's default.
- `SandboxPort` takes one step for a driver. This crate has no driver and sends none. The port hands over no stored finding, which is why endpoint inference runs inside `web_capture`.
- The sink refuses a finding of a kind or a level the manifest does not declare. Nothing checks what is found against a schema.
- Consent is checked by `a2m-actions` before a discoverer is called.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| A runner, inside the engine's sandbox | Each discoverer, for discovery jobs that `a2m-actions` submits for the actions in the table above, `discover.run`, and `scout.run` |
| `a2m-actions`, through the registry | The describer, for `finding.describe` and the scout's report |
| `a2m-pack-web-ui`, through stored findings only | Snapshot findings, for its drafting rules and screen matching |
| `a2m-pack-http`, through stored findings only | Endpoint findings, for its drafting rule |

It uses the base's `Observer`, `Look`, `Snapshot`, the list of what an observing tab refused, and the page's own GET exchanges an observing tab recorded, which it reads through the provider contract's traffic capture (`take_capture` over the tab as a `TrafficCapture`, with the discovery's redactor; [decision 0391](../../decisions/0391-traffic-capture-is-a-capability-of-the-provider-contract-that-yields-structure-only.md)), so that what reaches endpoint inference is the contract's `CapturedRequest` and has passed its check; the redactor on everything it finds. The base's platform profiles are not built.

## State

| State | Where |
| --- | --- |
| One observing tab for each discovery | Memory, until the discovery ends |
| A browser profile of the engine's own, `discovery` | The base's profile root, for each target and environment |
| A capture in progress: its watching tab, the requests and screens taken so far | Memory, until the capture ends; its bound is the handle's |
| A browser profile of the engine's own, `watching` | The base's profile root, for each target and environment; a person who signs in in the watching window signs in there |
| Findings | The store, through the sink. No body is kept |

## Failures

| Class | Code | When |
| --- | --- | --- |
| `edge_blocked` | `forbidden_by_edge`, `rate_limited_by_edge` | A page answered 403 or 429. `web_probe` names the block in its surface finding; the other discoverers stop. `challenge_page` is not built |
| `session_expired` | `needs_login` | A page asks for a sign-in: it answered 401, the tab ended at another origin, or a password input shows. `web_probe` makes a finding of it; the other discoverers stop, and a page a link led to is counted and passed over |
| `policy` | `surface_not_declared` | Asked to look at an address that is not of the surface's origin |
| `policy` | `address_looks_like_an_action` | Asked to visit an address whose path or query holds a word of `ACTION_WORDS`, by the surface, a focus, or a link; or a redirect ended at one |
| `input` | `focus_invalid` | The focus `paths` is not a list of paths that each start with one `/` |
| `transient` | `page_not_loaded`; the base's `timeout` and `connection_lost` | The page did not load, or the browser was lost |
| `busy` | The base's `browser_unavailable` | No browser is installed |
| `unknown_variant` | `page_unreadable` | The page could not be read in a way no rule covers. The message names the kind of answer and holds none of the browser's or the page's text |

`unknown_platform` is not a failure in discovery: the surface finding says so, and its tier is `unsupported` until a person names a profile.

Hitting a bound is not a failure. The report names the bound, `pages` or `seconds`, and counts the pages left unread.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| No write reaches the test application during any discovery | A page that writes when it loads, a page whose worker writes, a page that opens a socket to a route that writes, and links that look like an action by address and by name. The test first shows that each route writes when it is reached; then the control interface counts no write and holds what it held |
| Two discoveries of the same application in the same state give the same findings | Each discoverer run twice: the same identifiers. Package E5 does it by hash for the whole scout |
| No registered secret appears in a finding or a description | A secret planted in a record, a heading, a link, a control, a cell, a JSON value, a JSON name, and a query |
| No finding is above `discovered`, and each has a coverage and a limitation | Checked on every finding of the scouting test |
| An endpoint with a tier-two segment is `inferred`; an endpoint never answered is `inferred` | Unit tests on templating, and the capture of two orders with `tier_two` |
| A discovery that hits a bound says which, and how many pages it left | A discovery whose time is up |
| A sign-in wall and a block are named and never read | A page that redirects to a sign-in form; a page that answers 403 |
| An address that looks like an action is never visited, whatever named it; a redirect to another origin is a sign-in by redirect | `an_address_that_looks_like_an_action_is_never_visited_whatever_names_it`: two focus paths, by path and by query, for both discoverers that take a focus, and a surface whose own address is one; the control interface counts no write |
| A failure message holds no page text | `a_page_that_cannot_be_read_puts_none_of_its_text_in_the_message` |
| A platform the profiles do not know is reported unknown, never guessed | The probe of the test application: `unknown_platform`, tier `unsupported` |
| A page that shows an order gives the snapshot the web pack's drafting rules are proven against, the same twice | `a_page_that_shows_an_order_gives_the_screen_the_web_pack_drafts_from_the_same_twice`: the finding equals the one `a2m-pack-web-ui` keeps in `tests/fixtures/findings/` ([decision 0183](../../decisions/0183-the-web-pack-drafts-a-read-of-each-table-a-snapshot-names.md)) |
| A registry with only this pack builds, and its digest is stable | Two registries |

Run on Linux only. The crate has no code that differs by operating system.

## Build order

1. Done, package 2.6: `web_auth`, against case 13's sign-in variants (`each_sign_in_variant_of_case_13_is_classified_and_nothing_is_sent`); the application is sent no form and no code. The recommendation it feeds is `a2m-auth`'s and `auth.detect`'s ([decision 0367](../../decisions/0367-auth-detect-recommends-a-mode-and-a-provider-by-fixed-rules-and-marks-it-inferred.md)), proven through the product by `crates/anything2mcp/tests/sign_in_detect.rs`; a move to another origin is recommended as `sso_oidc`, because the page cannot show a `SAMLRequest` form post.
2. Done, package E2: the manifest and registration; `web_probe`; `web_snapshot` with links followed within the bounds.
3. Done in part: `web_capture` of documents, refused requests, and the GET requests a page's own scripts make to its origin ([decision 0187](../../decisions/0187-the-web-pack-matches-a-screen-by-its-address-and-reads-by-key.md)), and endpoint inference with tier one and tier two, inside it. Done: screen matching's first half in `web_snapshot`, pages whose paths differ in one segment and whose tables and labelled values are the same given as one `screen` finding with a placeholder and `pages`; the names of a description list's terms and of the definitions a page labels are kept outside a record. Done: request matching in `web_capture`, `matched_requests` ([decision 0190](../../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md)). Not built: request shapes; a request's dependencies; scenario steps; cases 5 and 22.
3a. Done ([decision 0391](../../decisions/0391-traffic-capture-is-a-capability-of-the-provider-contract-that-yields-structure-only.md)): the pack offers traffic capture of `web` surfaces as a capability of the provider contract: its manifest declares `points.traffic_capture`, it registers a `CaptureSource` that opens an observing tab on the surface's own address and records before the page loads, and `web_capture` reads its tab's requests through `take_capture`. A request the contract's check withholds is a `not_exercised` limitation of its page. Test: `a_capture_of_a_pages_own_requests_yields_structure_and_no_planted_secret`. No action handler opens the capture yet.
4. Not built: dependencies between requests and the front-end logic mark.
5. Done ([decision 0442](../../decisions/0442-a-persons-use-is-watched-by-web-use-and-what-it-saw-twice-is-drafted-held.md)): `web_use`, the capture of a person's own use, over the base's watching tab (package E2c). The design is [decision 0392](../../decisions/0392-a-page-that-reads-by-a-method-that-may-write-is-learned-from-a-persons-watched-use-and-read-through-an-asserted-request.md): the capture is a discoverer a person drives (`driven_by`), it runs in the watching tab and nowhere else, it takes a snapshot of each screen the person visits, and it drafts a request by a method that may write as a held endpoint with a question (packages E2a and E2).
6. Not built: platform profiles and the support tier, which are data of the base.
7. Not built: `web_declared`, with brief 24.

## Open points

- How a GET that changes state is recognized ([section 402](../../spec/4-building-tools/402-the-scout-and-who-does-what.md)). A word list is a start, not an answer.
- Whether the neutral part of the probe belongs in the kernel ([section 202](../../spec/2-kernel/202-kernel-and-packs.md)): naming a surface's kind and its pack before any pack is chosen.
- How this crate is held to the `endpoint` finding schema's version when `a2m-pack-http` changes it. Nothing checks what is found against a schema today; the crate writes `format` 1 and the owner's seven names.
- A page inside an application that holds a password input is taken for a sign-in wall and is not read.
- When a page is ready to be read. A page is read when its document is parsed, or when the focus's `ready_text` shows. A page that fills itself by script later is read early, and nothing says so.
- Whether a discovery should look through a session marked for discovery. The scope carries none.
- **One profile, `discovery`, serves every person's discoveries of a target and environment.** Cookies and storage a page sets persist from one discovery to the next, so two discoveries can differ by what an earlier one left, and a profile signed in by some means of the system's own would no longer look as nobody. The browser engine starts with a cleared environment and an owned data directory, so a device's account is not handed to it; a fresh profile for each discovery would close the rest, at the cost of a browser start each time.
- A page title in a `screen` finding is redacted and cut to 160 characters; it is free text from the system, which the describer keeps apart and which no field marks untrusted yet.
- The sandbox port takes steps for a driver. A discoverer that looks through a browser of its own pack has no use for it, and one that needs stored findings cannot get them.
- The file format of a scenario ([section 401](../../spec/4-building-tools/401-overlaying-tools-on-an-existing-system.md)).
- Whether capture should also read what in-page code sends over WebSockets and server-sent events, which section 301 observes for confirmation and does not drive.
