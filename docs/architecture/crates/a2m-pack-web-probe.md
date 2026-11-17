# `a2m-pack-web-probe`

The web pack's discoverers and its describer: the capability probe, sign-in detection, snapshots, traffic capture, endpoint inference with two-tier path templates, and what a site declares for agents. It looks at a web application through an engine-owned browser and returns findings. It cannot write, and the way it is built makes sure of that.

## Facts

| | |
| --- | --- |
| Side | Pack: web |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5, and `a2m-pack-web-browser` |
| Depends on | `a2m-core` (`Finding`, `Subject`, `Coverage`, `EvidenceLevel`), `a2m-provider` (`Discoverer`, `Describer`, `SandboxPort`, `PackContext`, registration), `a2m-auth` (`SessionLease`, read-only use), `a2m-telemetry`, `a2m-pack-web-browser` (an `Observer`, never `Browsers`). Outside: `tokio`, `serde_json` |
| Built in | Package E2, after E1 gives the discoverer extension point and the findings store, and after 2.6 gives sign-in. `auth.detect` arrives earlier, in package 2.6, as the first discoverer |
| Carries out | Spec sections [13](../../spec/13-interface-flavors.md) (the probe), [15](../../spec/15-identity-sign-in-and-sessions.md) (auth discovery), [26](../../spec/26-agent-toolkit-how-tools-get-built.md) (capture, mining, read-only exploration), [58](../../spec/58-discovery-and-findings.md), [60](../../spec/60-overlaying-tools-on-an-existing-system.md) (scenarios, templating), [67](../../spec/67-the-scout-and-who-does-what.md) (stages 1 and 2 for web surfaces) |

## Owns

- Saying what a web surface is: platform, framework, support tier, sign-in method, and candidate surfaces it plainly offers.
- Snapshots as findings: screens, fields, and actions seen, each with its subject and coverage.
- Capture: exchanges seen while a scenario runs or a person uses the system, kept as shapes, attributed to scenario steps.
- Endpoint inference: grouping exchanges into endpoints, templating paths in two tiers, inferring request and response shapes and dependencies between requests.
- Reading what a site declares for agents, including tools a page registers, as untrusted findings.
- The describer: showing an agent, in compact text, what was seen.

It must never: hold an acting tab; press, type, or submit; send a request other than a navigation to an address it was given or found as a plain link; enter a surface the target did not declare; keep a value where a shape will do, unless a person asked and policy allows it; claim `discovered` for anything that did not answer; write a draft file, which is the drafter's work.

## What it registers

| Registration | Content |
| --- | --- |
| Modes, drivers, surface kinds, file kinds, lowerings, lint rules, auth providers | None. The surface kind `web` is registered by `a2m-pack-web-ui` |
| Finding kinds | `surface`, `sign_in_method`, `screen`, `field`, `action`, `exchange`, `declared_tool`, each a checked name with the schema of its `found`. It produces `endpoint` findings and does not own that kind |
| Discoverers | `web_probe`, `web_auth`, `web_snapshot`, `web_capture`, `web_endpoints`, `web_declared`, below |
| Describer | `web_describer` |
| Drafting rules | None of its own. Its findings are drafted by the rules of the crates that own the file kinds: screens by `a2m-pack-web-ui`, endpoints by `a2m-pack-http` |

| Discoverer | Action that runs it | Reads | Finding kinds | Level |
| --- | --- | --- | --- | --- |
| `web_probe` | `target.probe` | A page load: globals, page markers, script names, headers, cookie names, matched against each profile's detection rules | Surface, with platform, framework, confidence, support tier | `discovered` for what answered; the platform is `inferred` with its confidence |
| `web_auth` | `auth.detect` | Redirects, challenges, a request for a client certificate, the identity provider's discovery document, whether backend calls carry a cookie or a bearer token | Sign-in method, with a recommended mode | `inferred`, with the observations as evidence |
| `web_snapshot` | `screen.snapshot` | The accessibility tree, through the base's snapshot builder | Screen, field, action | `discovered`; coverage `partial`, limitation `single_variant` |
| `web_capture` | `capture.start`, `capture.stop`, and scenario runs | Exchanges, by structure | Exchange | `discovered` |
| `web_endpoints` | `endpoints.infer` | Stored exchange findings, not the page | Endpoint, with its template, shapes, and dependencies | The endpoint `discovered` if it answered; its template `inferred` |
| `web_declared` | `discover.run`, the scout | A site's declarations for agents; tools a page registers | Declared tool, declared capability | `inferred`, source `published_description`, marked untrusted |

The `endpoint` finding kind is owned by `a2m-pack-http`, which registers the schema of its `found`. This crate declares that it produces that kind, and the sink validates each one against the registered schema. If the HTTP pack is not enabled, `web_endpoints` is not available and says why. Packs share the shape as data, never code: this crate does not know the endpoint file format.

## Discovery cannot write

Rule one of [section 58](../../spec/58-discovery-and-findings.md) is held in four places. Each would hold without the others.

| Where | How |
| --- | --- |
| The type | This crate is constructed with an `Observer` from the base. An `Observer` opens observing and watching tabs only; neither has an input method, and there is no way to obtain an acting tab from one. A check in CI refuses the names `ActTab` and `Browsers` in this crate's source |
| The browser | An observing tab fails every request whose method is not GET, HEAD, or OPTIONS before it leaves the browser, from the page, its workers, and its frames, unless a person marked that request safe. WebSockets are refused at creation. Each blocked request becomes a limitation on the finding |
| Navigation | A link is followed only when it is a plain link to a declared surface, it is not a form, and its address does not look like an action by the pack's list of such words (sign-out, delete, remove, approve, confirm, send, pay, and the like). A refused link is counted, never silently dropped. What "looks like an action" means is settled in Phase 4 |
| The kernel | Discovery runs as a job on a runner, inside the engine's sandbox, with a lease the keeper marked for discovery. A discoverer is given only what `SandboxPort` offers: no driver, no ledger token, no way to reach `Driver::act` |

**Capture of a person's use is not exploration.** When a person starts a capture, they use the system in a visible window through a watching tab, and may save. That save is the person's act; the discoverer only listens. The scout never opens a watching tab, and a capture can only be started and stopped by the person whose use it records ([section 61](../../spec/61-effects-hazards-and-consent.md)).

## Endpoint inference

| Step | Rule | Level |
| --- | --- | --- |
| Group | By method, origin, and path template. Requests the browser made for the page's own assets are left out by type | The group `discovered` |
| Template, tier one | A segment becomes a parameter only when it is an identifier by format: a number, a universally unique identifier, a long hexadecimal string, a date. Every literal seen is kept beside it | `inferred` |
| Template, tier two | A segment that varied between otherwise identical requests. Only when the caller asks for it | `inferred`, limitation `sampled`, marked as resting on variance alone |
| Shapes | Names and types from request and response bodies, merged across exchanges; a field seen in some responses and not others is optional with limitation `sampled` | `inferred` |
| Dependencies | A value in a request that equals a value in an earlier response of the same capture: a token or an identifier the request depends on. Found by comparing digests of values, so no value is stored | `inferred` |
| Binding to an action | The scenario step during which the request was sent. A request seen in two steps is ambiguous and is not bound | As the scenario |
| Front-end logic | A request whose body holds a field no input explains, or a header that changes per request with no source found, is marked as possibly computed or signed in the browser. Its endpoint is drafted for `page_http` only | `inferred` |

A derived finding is never stronger than its weakest parent: one tier-two segment makes the whole endpoint `inferred`.

## Interface

A sketch.

```rust
/// Called once by the product crate, with the pack context and the observer of the one `Browsers`.
pub fn register(reg: &mut Registration, ctx: &PackContext, observer: Observer) -> Result<(), Failure>;

pub struct WebProbe { observer: Observer }
impl Discoverer for WebProbe {
    fn name(&self) -> &str;                                       // "web_probe"
    fn suits(&self, surface: &Surface) -> bool;                   // surfaces of kind web
    async fn discover(&self, req: &DiscoverRequest, port: &SandboxPort) -> Result<DiscoverReport, Failure>;
}
// WebAuth, WebSnapshot, WebCapture, WebEndpoints, WebDeclared: the same trait.

pub struct Capture { /* an observing or watching tab, the scenario step in force, counters */ }
impl Capture {
    pub fn step(&mut self, name: &str);                           // attributes what follows to this step
    pub async fn stop(self) -> Result<CaptureSummary, Failure>;   // counts kept, dropped, refused
}

pub struct WebDescriber;
impl Describer for WebDescriber {
    /// Snapshots in the text form of the reference; captures as a list of exchanges by step;
    /// endpoints as method, template, and shapes. Every string from the target is marked untrusted.
    fn describe(&self, findings: &[Finding], limits: &DescribeLimits) -> Description;
}
```

What the kernel gives this crate:

- `DiscoverRequest` carries the surface, the bounds (time, size, depth, request count), the target's rate limit, and a `SessionLease` or none. A surface that needs a sign-in and has no session is reported as `needs_login`, and the scout carries on with the others.
- `SandboxPort` takes findings, refuses one above `discovered` or one whose `found` does not fit its kind's schema, computes its `id`, and has it stored. It also hands over stored findings, such as exchanges for `web_endpoints`. This crate never stores anything itself.
- Consent is checked by `a2m-actions` before a discoverer is called.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| A runner, inside the engine's sandbox | Each discoverer, for discovery jobs that `a2m-actions` submits for the actions in the table above, `discover.run`, and `scout.run` |
| `a2m-actions`, through the registry | The describer, for `finding.describe` and the scout's report |
| `a2m-pack-web-ui`, through stored findings only | Snapshot findings, for its drafting rules and screen matching |
| `a2m-pack-http`, through stored findings only | Endpoint findings, for its drafting rule |

It uses the base's `Observer`, `Look`, `Snapshot`, exchanges, and platform profiles; the redactor on everything it describes.

## State

| State | Where |
| --- | --- |
| A capture in progress: its tab, step, counters | Memory, until stopped; a capture that outlives its bound is stopped and reported |
| Findings and their evidence | The store, through the sink. Bodies only when asked and allowed |

## Failures

| Class | Code | When |
| --- | --- | --- |
| `edge_blocked` | `challenge_page`, `rate_limited_by_edge`, `forbidden_by_edge` | The surface stops answering as itself. Discovery of that surface stops and a person is told |
| `session_expired` | `needs_login` | The surface needs a sign-in. Reported per surface |
| `policy` | `surface_not_declared` | Asked to look at an origin the target did not declare |
| `transient` | `timeout`, `connection_lost` | The page did not load, or the browser was lost |
| `unknown_variant` | `unknown_platform` | No profile matched. Not a stop for discovery: the finding says so, and the surface's tier is `unsupported` until a person names a profile |

Hitting a bound is not a failure. It is a limitation on the findings, `truncated` or `time_limit`, and the report names it.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| No write reaches the test application during any discovery | A page that posts on load, from a worker, over a WebSocket, and through a link that looks like an action; the control interface counts no write |
| Two scouts of the same application in the same state give the same findings | Package E5, by hash |
| No cookie, token, or password appears in a finding, a capture, or a description | The planted-secret test, run over a capture |
| An endpoint with a tier-two segment is `inferred` | Unit tests on templating |
| A capture that hits a bound says what it dropped | A capture with a tiny bound |
| A platform the profiles do not know is reported unknown, never guessed | A page with markers of no profile |

## Build order

1. Package 2.6: `web_auth`, against case 13's sign-in variants. It needs only page loads and redirects.
2. Package E2: `web_snapshot` and `web_probe`.
3. Then `web_capture` with scenario steps, and `web_endpoints` with tier one, against cases 5 and 22.
4. Then tier two, dependencies, and the front-end logic mark.
5. `web_declared`, with brief 24.

## Open points

- How a GET that changes state is recognized ([section 67](../../spec/67-the-scout-and-who-does-what.md)). A word list is a start, not an answer.
- Whether the neutral part of the probe belongs in the kernel ([section 55](../../spec/55-kernel-and-packs.md)): naming a surface's kind and its pack before any pack is chosen.
- How this crate is held to the `endpoint` finding schema's version when `a2m-pack-http` changes it.
- The file format of a scenario ([section 60](../../spec/60-overlaying-tools-on-an-existing-system.md)).
- Whether capture should also read what in-page code sends over WebSockets and server-sent events, which section 14 observes for confirmation and does not drive.
