# 301. The web pack

The web pack lets a target be a web application: something a person uses in a browser, through pages, forms, and the requests those pages send. It is the first pack to be built, and the one the kernel's first slices are proven with. It brings three modes: `ui`, which uses the page as a person would; `page_http`, which sends requests from inside the signed-in page; and `site_tools`, which calls tools a page registers for agents.

The pack is three crates. Their designs are the exact statement of how the code is arranged; this section says what the pack does and why.

| Crate | What it is | Design |
| --- | --- | --- |
| `a2m-pack-web-browser` | The base: a browser engine that speaks the DevTools protocol to Chrome, Edge, or Chromium, launched with a data directory the engine owns. It registers nothing | [Design](../../architecture/crates/a2m-pack-web-browser.md); the engine itself is [section 302](302-the-web-pack-browser-engine.md) |
| `a2m-pack-web-ui` | The drivers of the three modes, the widget drivers, applying platform profiles, the `screens/` file kind for web surfaces, the auth providers that need a browser, and the drafting rules for an observed interface | [Design](../../architecture/crates/a2m-pack-web-ui.md) |
| `a2m-pack-web-probe` | The discoverers and the describer: the capability probe, sign-in detection, snapshots, capture, endpoint inference, and what a site declares for agents. It cannot write | [Design](../../architecture/crates/a2m-pack-web-probe.md) |

The product crate builds one `Browsers` from the base and gives it to `a2m-pack-web-ui`. It gives `a2m-pack-web-probe` only the `Observer` of that `Browsers`, which can open tabs that look and nothing else. That one choice is what makes discovery unable to write (below).

## What it reaches, and what it refuses

| Reaches | How |
| --- | --- |
| A web application on a surface the target declares | A browser the engine launched, with a data directory the engine owns, one per identity |
| Requests the application's own pages send | From inside the signed-in page, with the page's own session, in `page_http` |
| Tools a page registers in the browser for agents | Called with typed arguments in `site_tools` |
| An application built on a browser runtime | It is a web surface. The supervisor starts its program by path and digest, and the pack drives it as it drives a browser it started. It fails if the application disables its inspector |

| Refused | Why |
| --- | --- |
| The user's everyday browser profile, or any browser process the engine did not start | Rule five. The engine never reads, copies, or drives another program's profile or cookie store ([section 215](../2-kernel/215-sessions-and-the-session-keeper.md)) |
| Hiding that it is automation, changing its fingerprint, answering a challenge | Rule five. A block is the named stop `edge_blocked` |
| Clicking by pixel or position, and XPath | Nothing improvises. Elements are found by role and name |
| A navigation or a request to an origin the target did not declare | Section [502](../5-security/502-defending-against-a-manipulated-agent.md): no input changes where a request goes |
| An unrecognized page, dialog, control, or platform | Rule seven. Each is a stop, never a guess |
| Pages that cannot be driven over the DevTools protocol, such as a browser's legacy compatibility mode | Unsupported tier. It would need a different back end |

## Modes

| Mode | Archetype | How it reaches the system | Needs | Best for | Main risk |
| --- | --- | --- | --- | --- | --- |
| `ui` | Interface | The browser engine acts on elements by role, label, text, or test id; no pixel clicks | A browser session | Writes that must pass the application's own validation; applications with no usable traffic | Layout and label changes |
| `page_http` | Request | Requests sent from inside the signed-in page with the page's own session, cookies, and headers | A browser session | Fast reads and proven writes without driving the interface | Front-end logic is skipped |
| `site_tools` | Request | Tools the site registers in the browser (WebMCP), called with typed arguments | A browser session | Sites that offer them | An early standard, offered by few sites |

The modes are those of [decision 0005](../../decisions/0005-seven-modes-behind-one-driver-interface.md), so the pack needs no new mode record. Each mode's place in the default order of bindings is its `rank` in the pack's manifest, as [section 201](../2-kernel/201-archetypes-and-modes.md) describes; the cheapest-first rule puts the interface last. An operation that began as `ui` steps gains a `page_http` or `http` binding safely through shadow and promotion (below).

`site_tools` belongs here and not to the MCP pack, because it needs a tab that can act and only the web pack has one ([decision 0062](../../decisions/0062-words-that-arrive-with-packs-are-registered-names-and-packs-share-data-never-code.md)). It shares ideas with `mcp`, a pinned list of tools and a typed call, and no code.

## What it registers

| Registration | Content |
| --- | --- |
| Surface kinds | `web` |
| Modes | `ui`, archetype interface; `page_http` and `site_tools`, archetype request |
| Step kinds | `goto`, `wait`, `locate`, `set`, `invoke`, `read`, `handle`, `upload`, `download`, `observe` for `ui`; `token`, `request`, `validate`, `extract` for `page_http`; `call_tool` for `site_tools`. Each a checked name within its mode |
| Drivers | One per mode, sharing one `Browsers` |
| File kinds | Owns `screens/` for surfaces of kind `web` ([file formats](../../reference/file-formats.md#screensyaml)). Reads `endpoints/`, which the HTTP pack owns ([section 303](303-the-http-pack.md)), for the `page_http` lowering |
| Binding fields | `ui`: `screen`, `verb`, `fields`, `action`. `page_http`: `endpoint`, `path_params`, `query`, `body`. `site_tools`: `tool`, `args` |
| Control names | Every control in the widget driver table below, as checked names |
| Finding kinds | `surface`, `sign_in_method`, `screen`, `field`, `action`, `exchange`, `declared_tool`. It produces `endpoint` findings, a kind the HTTP pack owns |
| Discoverers | Screen matching, in `a2m-pack-web-ui`; six in `a2m-pack-web-probe` (below) |
| Describer | `web_describer` |
| Auth providers | `form_login`, `sso_oidc`, `sso_saml` |
| Lint rules, drafting rules | Below |

`screens/` is also registered by the desktop pack, for surfaces of kind `desktop` ([section 305](305-the-desktop-pack.md)). Registration is keyed by file kind and surface kind, so the two do not collide.

If the HTTP pack is not enabled, a `page_http` binding does not compile, and the probe's endpoint inference is not available and says why. The pack compiles the `endpoints/` entries it is handed into its own step payload; no request-template type is shared between packs.

## Verbs and how they lower

| Verb | `ui` | `page_http` | `site_tools` |
| --- | --- | --- | --- |
| `open` | Yes | No | No |
| `read` | Yes | Yes | If offered |
| `list`, `search` | Yes | Yes | If offered |
| `create`, `update`, `upsert` | Yes | Yes | If offered |
| `delete` | Yes | Yes | If offered |
| `upload` | Yes | Yes | No |
| `download` | Yes | Yes | No |
| `submit` | Yes | Yes | If offered |
| `call` | No | Yes | Yes |

`ui` lowers each verb into `goto`, `wait`, `locate`, `set`, `invoke`, `read`, `handle`, `upload`, `download`, and `observe`. `page_http` lowers into `token`, `request`, `validate`, and `extract`. `site_tools` lowers into `call_tool`. Each lowering compiles what it reads, its screens and, for `page_http`, the endpoint entries the compiler hands it as validated data, into its own opaque step payload, and marks the steps that commit and the steps that are lookups. An action with `commits: true` is a commit point; the compiler counts those after lowering ([section 206](../2-kernel/206-the-compiler-and-the-plan.md)). As built, `ui` declares the step kinds `goto`, `wait`, `read`, `set`, and `invoke`: each of the last three finds its own element and checks its fingerprint, and an `invoke` that commits is not done until the page shows the action's `confirm` or a declared variant, so a `ui` binding's confirm list is empty ([decision 0112](../../decisions/0112-the-ui-driver-as-built-five-step-kinds-and-a-commit-that-waits-for-its-confirmation.md)).

## The driver, part by part

| Mode | Prepare | Act | Confirm | Read back |
| --- | --- | --- | --- | --- |
| `ui` | A new acting tab in the session's browser, with the target's fixed environment | `goto`, `locate`, `set` through the widget driver, `invoke` | The action's declared `confirm`, often an `observe`d response | The field again after the screen is reached afresh, or the save response with `prefer: response` |
| `page_http` | An acting tab on the surface, at its declared ready page | `token` if declared, then `request` sent from inside the page | Status and response schema | The endpoint the binding names |
| `site_tools` | An acting tab, and the list of tools the page registers, compared with the binding's tool and schema digest | `call_tool` with typed arguments | The tool's typed result | The read tool the binding names, or the `ui` field. Without one the write is `irreversible` |

The rules every driver keeps are the kernel's ([section 207](../2-kernel/207-the-engine-loop.md)). For this pack they mean:

- **No retries inside a step.** A widget driver's escalation from fill to key-by-key typing to the native value setter is one strategy in one step, chosen by the control, not a retry.
- **Every failure says whether input reached the page.** The base reports each act as sent, not sent, or unknown. A crash or a disconnect after an `invoke` reached the page is unknown, and for a commit point that is `possible_commit`, never `transient`.
- **An element is valid for one page generation.** A navigation or a re-render that removes the node makes it stale, and using it is a failure, never a silent re-query.
- **An outcome can carry outputs known before the commit point,** such as a value the form shows before saving. The engine records them before the committing step runs.
- **A driver never signs in or renews.** It reports a sign of expiry through the lease and stops; the session keeper decides.
- **The fixed environment never varies:** viewport, locale, time zone, color scheme, and reduced motion are set per target.

A `ui` step costs under 50 ms of overhead and a `page_http` step under 20 ms, on a warm tab. These are budgets until measured ([section 225](../2-kernel/225-performance.md)).

## Locating by role and name

Elements are found by meaning: from the accessibility tree and a small helper injected into an isolated world the page's own scripts cannot read or change. A field or action in a screen model carries an ordered list of locators, tried in order; the first that matches exactly one element and passes the fingerprint wins.

| Locator | Matches by |
| --- | --- |
| `{role, name}` | Accessibility role and accessible name. The first choice |
| `{label}` | The text of the label associated with the control |
| `{placeholder}` | Placeholder text |
| `{text}` | Visible text, exact unless `contains: true` |
| `{test_id}` | A test identifier attribute, when the application has stable ones |
| `{title}` | Title or tooltip text |
| `{css}` | A selector. Allowed only last in the list, and flagged by the linter |

- **An ambiguous match fails.** The driver never picks the first of several.
- **A fingerprint is required** on every field and action: what must also be true of the matched element, such as nearby text, its section, or its role. A match that fails it is `drift`.
- **Open shadow roots are pierced, and frames are declared.** A screen inside a frame declares its frame path. A closed shadow root falls back to the keyboard or the network.
- **Generated identifiers are rejected** by the surface's platform profile, which lists their patterns.
- Position alone, and XPath, are not accepted. The full format is in the [file formats](../../reference/file-formats.md#locators), and whether role and name find nearly every field is hypothesis H-02.

## Interface flavors

Web applications are built in very different ways. Three mechanisms absorb the differences, so operation and pipeline files look the same whatever the application is built with.

- **The capability probe.** At setup it loads the application and matches detection rules (globals, page markers, script names, headers, cookie names) to name the platform, with a confidence. It assigns a support tier: full, read-only, API-preferred, or unsupported.
- **The platform profile.** One preset per platform: a readiness check, locator priority, frame and shadow scopes, patterns of generated identifiers to reject, the input commit strategy, and how often the platform's interface changes.
- **Widget drivers.** One tested driver per kind of control, each with its own rules for setting, reading back, and failing.

| Flavor | What makes it hard | Profile and driver behavior |
| --- | --- | --- |
| Server-rendered pages | Each action reloads the page | Wait on load; forms carry their own hidden tokens |
| ASP.NET WebForms, JSF | Generated identifiers; partial postbacks re-render the form | Locate by label; wait until the asynchronous postback reports it has finished |
| React | Setting `value` directly never reaches the application's state | Escalate to the native setter plus events ([reference](https://coryrylan.com/blog/trigger-input-updates-with-react-controlled-inputs)) |
| Angular | Asynchronous change detection | Wait on the Testability stable signal when the application exposes it ([Angular](https://angular.dev/api/core/Testability)) |
| Enterprise component frameworks whose identifiers depend on load order | Identifiers such as a numbered button shift between loads | Control-level locators by view and control type; never generated identifiers |
| Component platforms with shadow trees, frames for older pages, and several releases a year | Hidden from ordinary selectors; frequent change | Role and label locators; frame paths; drift checks more often |
| Low-code platforms that draw an application inside a host frame | Applications inside a host frame; grids addressed by row index | Locator order: role, label, test id, control name |
| Virtualized grid components | Only the visible rows exist in the page | The grid driver addresses a cell by row key and column id, scrolling or filtering to find the row; reads prefer the network |
| Shadow DOM | Hidden from ordinary selectors | Open roots are pierced automatically; closed roots fall back to the keyboard or the network ([Playwright](https://playwright.dev/docs/locators)) |
| Flutter web | Drawn on a canvas; no elements by default | Turn on the semantics tree, then use it; otherwise read-only through the network ([Flutter](https://docs.flutter.dev/ui/accessibility/web-accessibility.md)) |
| Editors drawn on a canvas | Nothing to locate | API-preferred tier |
| Applications built on a browser runtime | Not in a browser tab | Started by the supervisor and driven over the protocol; fails if the application disables its inspector |
| A browser's legacy compatibility mode | Not drivable over the DevTools protocol | Unsupported tier |
| Pop-ups, new tabs, downloads | The action leaves the page | Declared as an expected event on the step; an undeclared one stops the run |
| A/B tests, feature flags, languages | The same page differs between sessions | Fixed locale; ordered locators; an unknown variant stops the run |

Several large business platforms and grid component libraries have not been checked against their own documentation. Their profiles start in the read-only tier until tested.

### Platform profiles

A profile is data, compiled into `a2m-pack-web-browser` and versioned with it, because both the drivers and the probe need it. Applying a profile to locating and input is `a2m-pack-web-ui`'s work; choosing one for a surface is the probe's. Code never branches on a platform's name where a profile's data can say it.

```yaml
profile: aspnet_webforms
detect:                       # detection rules: globals, page markers, scripts, headers, cookies
  any:
    - {dom: "input[name='__VIEWSTATE']"}
    - {js: "Sys.WebForms.PageRequestManager"}
ready:
  js: "!Sys.WebForms.PageRequestManager.getInstance().get_isInAsyncPostBack()"
locators:
  priority: [label, role, text, css]
  reject_id_patterns: ["^ctl\\d+_", "^MainContent_"]
input_commit: blur_then_wait_ready
frames: {mode: declared}
drift_risk: low               # how often the platform ships interface changes
support_tier: full
```

| Field | Meaning |
| --- | --- |
| `detect` | Rules that name the platform, with a confidence. No match is `unknown_platform` |
| `ready` | The platform's own readiness rule, used by the condition `{profile_ready: true}` |
| `locators` | The locator priority, and the identifier patterns the linter rejects (`web_generated_id`) |
| `input_commit` | How a value is committed in the page |
| `frames` | Whether frames are declared or found |
| `drift_risk` | How often the platform changes its interface; it sets how often drift is checked |
| `support_tier` | `full`, `read_only`, API-preferred, or `unsupported`. A write binding on a `read_only` or `unsupported` surface does not compile (`web_profile_readonly`) |

A surface whose profile is `auto` and matches none is `unknown_platform`: the probe reports it, its tier is `unsupported` until a person names a profile, and a run on it stops.

### Widget drivers

One driver per kind of control. The control names are shared with the desktop pack where the control exists in both; the code is not ([section 305](305-the-desktop-pack.md)).

| Control | Set | Read back |
| --- | --- | --- |
| `text`, `textarea` | Fill; escalate to key-by-key typing, then the native value setter with events | Input value |
| `number`, `currency`, `measurement` | As text, formatted by the field type | Input value, normalized |
| `select` | Choose by stored code | Selected code |
| `combobox`, `autocomplete` | Open, type to filter, choose the option whose code or exact label matches | Selected code, or the label map |
| `date`, `datetime`, `time` | Type in the field's declared format; fall back to the picker | Value, normalized to the field's time-zone rule |
| `checkbox`, `toggle` | Set to the target state only if different | Live checked state, never the attribute |
| `radio` | Choose by stored code | Checked option |
| `slider` | Keyboard steps to the value | Current value |
| `file` | Attach the file to the input directly | File name shown |
| `rich_text` | Editor-specific command, or paste as plain text | Editor content, normalized |
| `grid` | Address a cell by row key and column id; scroll or filter to find the row | Cell value; the network response preferred |
| `tree` | Expand along a path of labels | Selected path |
| `tabs`, `accordion` | Activate by name | Active name |
| `dialog` | Declared as expected; confirm or dismiss by named action | Closed state |
| `toast` | Observed only | Text, used as a confirmation |

A password input's value is never read, whatever a step asks.

## Edge cases

Each common behavior of web applications has one built-in handling rule, so operation files declare the behavior instead of coding around it. Several of these rules are the kernel's and hold for every kind of system; they are listed here because web applications are where each was found.

| Behavior | Built-in handling |
| --- | --- |
| Pagination: pages, cursors, "load more", infinite scroll | The `list` verb with a pager per style; results paged by cursor to the caller |
| Long-running jobs inside the application, such as exports and reports | `call`, then poll a declared status until done or a time limit |
| File upload and download | Declared on the step. Files pass through a scoped staging folder, given as a checked path, never through the agent's context |
| Optimistic locking (ETag, version numbers, "changed by another user") | Read the version first and send it with the write; a conflict is `stale`, `version_conflict`, with both versions shown |
| Rotating anti-forgery tokens | A declared bootstrap request or page field supplies the token per request |
| Requests signed by front-end code | `ui` or `page_http` only; `http` is marked unsupported for that endpoint |
| WebSockets and server-sent events | Observed for confirmation in `ui` and `page_http`; not driven directly |
| GraphQL, including persisted queries | The endpoint catalog stores the operation name and query hash |
| Multi-step wizards and drafts | Pipeline steps with a checkpoint per page; resume returns to the last confirmed page |
| Workspaces and tenants | The workspace is part of the session; switching is an explicit operation |
| People editing the same record | Optimistic locking where available; otherwise read immediately before the write and compare |
| Maintenance windows and outages | A surface health check; calls queue, or fail as `transient`, `upstream_unavailable`, with a retry time |
| Locales, time zones, number formats | Fixed per target; field types normalize both sides ([section 210](../2-kernel/210-sources-and-value-normalization.md)) |
| Feature flags and A/B variants | Ordered locators and variant-aware screen models; an unknown variant stops |
| Bulk volumes | Pipelines batch by record, with rate budgets and a checkpoint per record |

## Large applications

A big system with many modules, services, and origins is one target with many surfaces, organized by module. These rules are the kernel's ([section 205](../2-kernel/205-target-files.md); parts and levels are in [section 401](../4-building-tools/401-overlaying-tools-on-an-existing-system.md)); web applications are where they are needed first.

- **Modules.** Files live under `targets/<target>/<module>/`, and tool names carry the module, such as `example_purchasing_order_create`. Agents find tools by module through search, so hundreds of operations stay usable.
- **Shared pieces.** Field types, sign-in, and environment are defined once per target and shared by all modules.
- **Several origins and services.** Micro-frontends, gateways, and separate services are separate surfaces with a declared relationship, such as sharing one sign-in.
- **Pipelines across targets.** A pipeline can read from one target and write to another, with each target's session and policy applied separately.
- **Load on demand.** Only modules in use are loaded, so a large target does not slow the server.

## Sign-in in a browser

The rules for sessions are the kernel's ([section 215](../2-kernel/215-sessions-and-the-session-keeper.md)): one live session per identity, one renewer, and the person's own account used with the person's own rights. The web pack supplies what needs a browser.

| The pack supplies | How |
| --- | --- |
| `sso_oidc`, `sso_saml` | Single sign-on in the engine's own profile. A person signs in once, in a visible window that shows its purpose; or, where the browser supports it, the engine's new profile signs in silently through the device's account (hypothesis H-05, brief 30) |
| `form_login` | A sign-in page with a stored credential, tried once. A refused credential is a stop, never a second try |
| The browser profile as the session's live state | A data directory owned by the engine, `profiles/<identity>/`, on the session's home runner, given to the base as a checked path ([decision 0031](../../decisions/0031-the-browser-always-runs-with-a-profile-the-engine-owns.md)) |
| Saved state | For a movable session, cookies and storage exported from the engine's own profile, sealed, and held by the keeper; restored after a restart or on another runner |
| Silent renewal | The keeper's first rung: loading the application in a background tab so the application's own silent renewal runs |
| Signs of expiry | Rules that recognize a sign-in page or a redirect to the identity host. The pack reports them; it acts on none |

- **Runs share a live session; they do not copy it.** Each run works in its own tab of the identity's one browser profile. Tabs share cookies and local storage, and each has its own session storage ([decision 0032](../../decisions/0032-one-live-session-per-identity-shared-by-tabs-with-a-single-renewer.md); hypothesis H-04).
- **Each identity has its own browser process and profile.** Two identities never share either.
- **Where the person signs in** is a visible window of the engine's own browser on one machine, and a sign-in page that shows the server runner's browser for a hosted user session ([section 503](../5-security/503-approval-and-sign-in-surfaces.md), [section 602](../6-running-it/602-identity-on-a-shared-server.md)).
- **Sessions that end with the browser.** Where an organization does not let a browser session survive closing the browser, the keeper keeps the browser running for the keep-alive window instead of saving and restoring state.
- **One session per account.** The probe checks whether signing in from the engine signs the person out of their own browser. If it does, the target is marked exclusive and the person is told before they first sign in.
- **On revoke** the keeper closes the tabs, calls the application's sign-out if one is declared, and has the base delete the profile directory and the saved state.

Other ways of signing in to a web application, such as a token from the operating system's broker for a backend the pages call, are held by providers in `a2m-auth` and used through `page_http` or `http`.

## The probe, capture, and endpoint inference

`a2m-pack-web-probe` looks at a web application through an engine-owned browser and returns findings ([section 218](../2-kernel/218-discovery-and-findings.md)). It never writes a draft; that is the drafter's work, by the rules of the crate that owns each file kind.

| Discoverer | Action that runs it | Reads | Finding kinds | Level |
| --- | --- | --- | --- | --- |
| `web_probe` | `target.probe` | A page load: globals, page markers, script names, headers, cookie names, matched against each profile's detection rules | Surface, with platform, framework, confidence, support tier | `discovered` for what answered; the platform `inferred`, with its confidence |
| `web_auth` | `auth.detect` | Redirects to an identity provider and its discovery document; `WWW-Authenticate` challenges; a request for a client certificate; whether backend calls carry a cookie or a bearer token, the token's audience and scopes decoded locally and never sent anywhere, and how the front end renews it | Sign-in method, with a recommended mode | `inferred`, with the observations as evidence |
| `web_snapshot` | `screen.snapshot` | The accessibility tree, through the base's snapshot builder | Screen, field, action | `discovered`; coverage `partial`, limitation `single_variant` |
| `web_capture` | `capture.start`, `capture.stop`, and scenario runs | Exchanges, by structure | Exchange | `discovered` |
| `web_endpoints` | `endpoints.infer` | Stored exchange findings, not the page | Endpoint, with its template, shapes, and dependencies | The endpoint `discovered` if it answered; its template `inferred` |
| `web_declared` | `discover.run`, the scout | A site's declarations for agents; tools a page registers | Declared tool, declared capability | `inferred`, source `published_description`, marked untrusted |

Screen matching, in `a2m-pack-web-ui`, sets a stored snapshot finding against the target's screen models. It reads findings, not the page. The describer shows an agent snapshots in the text form of the [steps reference](../../reference/steps-snapshots-and-diffs.md#snapshots), captures as exchanges by step, and endpoints as method, template, and shapes, with every string from the target marked untrusted.

Hitting a bound is not a failure. It is a limitation on the findings, `truncated` or `time_limit`, and the report names it. Two scouts of the same application in the same state give the same findings.

### Discovery cannot write

This is held in four places, and each would hold without the others.

| Where | How |
| --- | --- |
| The type | The probe is constructed with an `Observer`. It opens observing and watching tabs only; neither has an input method, and there is no way to get an acting tab from one. A check in CI refuses the names of the acting types in the probe's source |
| The browser | An observing tab fails every request whose method is not GET, HEAD, or OPTIONS before it leaves the browser, from the page, its workers, and its frames, unless a person marked that request safe |
| Navigation | A link is followed only when it is a plain link to a declared surface, it is not a form, and its address does not look like an action by the pack's list of such words (sign-out, delete, remove, approve, confirm, send, pay, and the like). A refused link is counted, never silently dropped |
| The kernel | Discovery runs as a job on a runner, inside the engine's sandbox, with a lease the keeper marked for discovery. A discoverer is given only what the sandbox port offers: no driver, no ledger token, no way to act |

In an observing tab:

| What could change the system | What happens |
| --- | --- |
| A request whose method is not GET, HEAD, or OPTIONS | Intercepted and failed before it leaves the browser, unless it matches the tab's safe requests, which only a person sets. Each one is reported as a limitation on the finding |
| A request from a worker the tab starts | A dedicated worker's requests pass through its page's interception, which is on before the page runs; any other child takes its own before it runs or is not let run; service workers are bypassed ([decision 0111](../../decisions/0111-an-observing-tabs-lock-as-built-and-what-the-browser-was-measured-to-do.md)) |
| A WebSocket | Refused at creation, and reported as a limitation |
| A beacon, a form submission, a download | Failed by the same interception, or refused |
| Navigation | Only to an address the caller passes, or a plain link as above |

### The watch tab and the explore tab

Exploring and capturing a person's own use are two different things, and the pack keeps them apart.

| | Exploring | Capturing a person's use |
| --- | --- | --- |
| Tab | An observing tab | A watching tab, in a visible window |
| Who acts | Nobody. Nothing can press, type, or submit | The person, who may save. That save is the person's act |
| Interception | Every request that could write is failed | None: the person is entitled to save, and nothing the engine sends goes through this tab |
| Started by | The scout, `discover.run`, or an authoring action | Only the person whose use it records, with `capture.start`; they stop it with `capture.stop` and are told what is kept ([section 213](../2-kernel/213-effects-hazards-and-consent.md)) |

The scout never opens a watching tab.

### Capture

A capture follows the rules of observation in [section 401](../4-building-tools/401-overlaying-tools-on-an-existing-system.md): one scenario per operation, every exchange attributed to the scenario step in force, the target's fixed environment, shapes by default and values only when a person asks and policy allows, and bounds that are counted. Capturing inside the engine's browser needs no system proxy and no installed certificate. Cookie, authorization, and set-cookie headers are removed by name before anything leaves the base, and every value the redactor knows is replaced. A capture that outlives its bound is stopped and reports what it dropped.

### Endpoint inference

| Step | Rule | Level |
| --- | --- | --- |
| Group | By method, origin, and path template. Requests for the page's own assets are left out by type | The group `discovered` |
| Template, tier one | A segment becomes a parameter only when it is an identifier by format: a number, a universally unique identifier, a long hexadecimal string, a date. Every literal seen is kept beside it | `inferred` |
| Template, tier two | A segment that varied between otherwise identical requests. Only when the caller asks for it | `inferred`, limitation `sampled`, marked as resting on variance alone |
| Shapes | Names and types from request and response bodies, merged across exchanges; a field seen in some responses and not others is optional, with limitation `sampled` | `inferred` |
| Dependencies | A value in a request that equals a value in an earlier response of the same capture: a token or an identifier the request depends on. Found by comparing digests of values, so no value is stored | `inferred` |
| Binding to an action | The scenario step during which the request was sent. A request seen in two steps is ambiguous and is not bound | As the scenario |
| Front-end logic | A request whose body holds a field no input explains, or a header that changes per request with no source found, is marked as possibly computed or signed in the browser. Its endpoint is drafted for `page_http` only | `inferred` |

A derived finding is never stronger than its weakest parent: one tier-two segment makes the whole endpoint `inferred`. The `endpoint` finding kind and its schema belong to the HTTP pack, which validates each finding the probe produces and drafts from it ([section 303](303-the-http-pack.md)).

### From mining to a verified binding

This is how an application with no documented interface gains fast request bindings.

1. **Capture** traffic inside the browser while the application is used.
2. **Infer** endpoints, as above.
3. **Draft** a `page_http` or `http` binding beside the `ui` binding of the same operation.
4. **Shadow.** While the `ui` binding does the real work, the new binding predicts the request the page will send, without acting. Matches and differences are recorded (`shadow.report`); fields an endpoint marks `volatile` are ignored in the comparison.
5. **Promote** after a set number of exact matches and a person's approval (`binding.promote`). Reads can be promoted quickly; writes need more evidence.

Mining can stop short. If the front end computes values before saving or signs its requests, the request binding stays in shadow or is marked unsupported, and the `ui` binding remains the way in. Whether comparing shapes in shadow catches the front end's logic is hypothesis H-10.

## Lint rules

Codes are stable once released.

| Code | Level | Rule |
| --- | --- | --- |
| `web.css_not_last` | Error | A `css` locator is allowed only last in a list |
| `web.css_used` | Warning | Any `css` locator |
| `web.generated_id` | Error | A locator matches a rejected identifier pattern of the surface's profile |
| `web.no_fingerprint` | Error | A field or action with no fingerprint |
| `web.commit_unconfirmed` | Error | An action with `commits: true` and a confirmation that could be true before it acted |
| `web.variant_unhandled` | Error | A variant with no handling |
| `web.frame_undeclared` | Error | A screen whose locators need a frame and declare none |
| `web.profile_readonly` | Error | A write binding on a surface whose profile's tier is `read_only` or `unsupported` |
| `web.signed_request` | Error | A `page_http` binding on an endpoint marked as signed in the browser, used for a write before it is verified |

## Drafting rules

| From a finding of kind | The drafter writes | Left as an open question |
| --- | --- | --- |
| `screen`, from a snapshot | A screen model: `reach` from the observed address with identifier segments as placeholders, `ready` from the profile's rule or a visible heading | What the screen is for; its name beyond the mechanical one |
| `field`, from a snapshot | A field: `control` from the role by a fixed table (textbox to `text`, combobox to `combobox`, checkbox to `checkbox`); `locate` as role and name, then label; `fingerprint` from the nearest heading or section | Its field type. A field type is meaning, so the draft does not compile until it is settled |
| `action`, from a snapshot | Nothing | Every action, and whether it commits |
| A tool a page registers, marked read-only by the page | A `site_tools` binding for one read operation | Tools not marked read-only; whether the page's marking is believed |

Endpoint findings are drafted by the HTTP pack's rule. Whether drafts by rule save work is hypothesis H-31.

## Effects and hazards

| Effect | Declared as |
| --- | --- |
| Starts a process | The browser program, by path and digest, through the supervisor the pack's context scopes. A browser whose reported version or digest differs from what was launched is closed |
| Reads and writes files | The profile root, and a staging folder for uploads and downloads, each as a checked path |
| Uses the network | The target's declared web surfaces, for what the engine itself sends |
| Holds a session | For surfaces of kind `web` |
| Changes what is on a person's screen | Only for a sign-in window, or a watching tab the person opened |

All are `enforced`. Hazards come from operations, not from the driver ([section 213](../2-kernel/213-effects-hazards-and-consent.md)).

The pack masks restricted and password values in every snapshot and screenshot it takes, before either leaves the pack. Exchanges leave the base redacted, and bodies are kept only when a filter asks for them.

## Failures

| Class | Codes | When |
| --- | --- | --- |
| `drift` | `locator_not_found`, `fingerprint_mismatch`, `locator_ambiguous`, `response_schema_mismatch` for `page_http`, `upstream_tool_changed` for `site_tools` | The page no longer matches its model |
| `unknown_variant` | `unexpected_dialog`, `unexpected_page`, `unexpected_status`, `unknown_platform` | An undeclared dialog, page, or status; a surface whose profile is `auto` and matched none |
| `rejected` | `app_validation_error` | A declared variant with handling `stop` that shows a validation message |
| `possible_commit` | `no_confirmation` | A commit point was sent, or its sending is unknown, and its confirmation did not arrive |
| `stale` | `version_conflict` | The version precondition failed |
| `edge_blocked` | `challenge_page`, `rate_limited_by_edge`, `forbidden_by_edge` | The edge answered instead of the application. Never retried; a person is told ([section 302](302-the-web-pack-browser-engine.md)) |
| `session_expired` | `needs_login` | Reported through the lease, after the keeper has the signal |
| `policy` | `surface_not_declared`, `effect_not_declared` | A navigation or in-page request to an origin the target did not declare; a browser program the pack's manifest does not declare, which the scoped supervisor refuses |
| `busy` | `browser_unavailable` | The browser is not installed |
| `transient` | `timeout`, `connection_lost` | Before anything was sent |

`locator_ambiguous`, `unknown_platform`, and `browser_unavailable` are proposed codes, settled as the crates are built. In discovery, `unknown_platform` is not a stop: the finding says so. `edge_blocked` stops discovery of that surface, and `needs_login` is reported per surface while the scout carries on with the others.

## The test stand

The test application ([test application plan](../../plan/test-application.md)) is the pack's test stand: an imaginary order-entry application, the `example` target, with a control interface that reports what it stored and how many times, and faults that can be switched on. Its cases 1 to 27 are the web pack's. A page that registers tools, for `site_tools`, is a new case.

| Cases | What they prove for this pack |
| --- | --- |
| 6 to 12 | Locating without generated identifiers, widget escalation, in-page navigation, grids by key, custom controls, declared dialogs, frames and shadow trees |
| 13 to 15 | The auth providers, sign-in detection, the keeper's probes, exclusive sessions |
| 16 to 21 | Normalization, `possible_commit` after a crash, identity strategies, dependent records, no duplicate send, `changed_since_preview` |
| 22 to 24 | Mining stopping at shadow; edge blocks; long jobs and downloads |
| 25 to 27 | Variants, locale, untrusted text |

The driver conformance suite is the measure any implementation of the `ui` driver must pass: launching, locating, waiting, acting, frames and shadow trees, network, snapshots, isolation, and speed. A different engine behind the same driver interface is judged by the same tests (hypothesis H-01). Discovery is tested with pages that post on load, from a worker, over a WebSocket, and through a link that looks like an action; the control interface must count no write.

## The first kinds of system to support

Three kinds of first target prove the design in different situations. The web application is this pack's; the other two are the HTTP or MCP pack's and the desktop pack's, and are listed here so the three can be compared.

| | A web application with no known interface | A system with a published interface | A desktop application |
| --- | --- | --- | --- |
| Starting mode | `ui`, with `page_http` and `http` bindings mined and promoted over time | `api`, or `mcp` pass-through if the system's own MCP server covers the job | `desktop` |
| Identity | The user's own sign-in in an engine-owned profile on one machine; a service account or per-user modes on a server | A token or OAuth from the vault | The signed-in desktop user |
| First pipeline | Enter the rows of a spreadsheet as records | Create or update records from a source list | A read-only export |
| Record key | A code that is unique within its parent record | The record id, or a custom field holding the source id | Specific to the application |
| Hardest part | Units, fractions, and rounding in read-back; values recalculated after save | Rate limits; features the interface lacks | The quality of the application's accessibility data |

The first thing to learn about a web application costs ten minutes: open it in a browser, open the developer tools' network view, save a record, and see whether the page sends JSON to an interface, posts HTML forms, or neither. That answer decides how far the request modes can go.

Wrapping any vendor's software should be cleared with the vendor's terms and with IT before it is used on real work. Target files for real applications are kept outside this repository, in a private location the owner chooses.

## The entry bar

The eight items of [section 202](../2-kernel/202-kernel-and-packs.md).

| # | Item | The web pack's answer |
| --- | --- | --- |
| 1 | A driver for its verbs, no retries inside | One driver per mode, over one `Browsers`. Widget escalation is one strategy in one step |
| 2 | A read-back rule for every write | As in the driver table. A `site_tools` write without a read tool or a `ui` field is `irreversible` |
| 3 | Each identity strategy | `natural` and `composite`: `reach` built from the key. `stamped`: write the stamp field, then find the record through a list screen. `find`: a search screen, with the declared `settle`. `none`: the write is `irreversible` |
| 4 | A test stand | The test application, cases 1 to 27, and a new case for a page that registers tools |
| 5 | A discoverer | The six of the probe, and screen matching |
| 6 | Failures mapped | The table above |
| 7 | Effects and hazards | The table above; hazards come from operations |
| 8 | A record per new mode | None needed: the modes are from decision 0005 |

## Build order

The crates are built in the order their designs give: the browser base chosen by a spike in slice 0.4 (brief 1), `ui` reading one field in slice 1.1, registration with the compiler in slice 1.3, every read verb in slice 1.6, the auth providers in packages 2.3 and 2.6, writes from slice 3.3, `page_http` in package 4.2, the probe in package E2 with `web_auth` first in package 2.6, the drafting rules in package E4, and platform profiles, the remaining widgets, and `site_tools` in Phase 8.

## Open points

- The base library behind the browser engine, and whether it can use a pipe for the protocol (slice 0.4, hypotheses H-01 to H-03).
- How a program built on a browser runtime is started so that the protocol runs on a pipe, as for a browser.
- Whether the shared parts of `screens/` should become the interface archetype's, in the kernel, once the desktop pack exists (hypothesis H-37).
- How the `page_http` lowering and the probe are held to the version of the `endpoints/` and `endpoint` finding schemas they were written for, when the HTTP pack changes them.
- The tool registry a page offers is an early proposal (brief 24); its shape is settled when the standard is.
- Whether a validation message is always a declared variant, or a profile can name a generic one.
- How a GET that a page treats as an action is recognized. A word list is a start, not an answer; it is settled in Phase 4.
- Whether observing and acting tabs in one browser are safe enough together, or an observing scout should use its own browser. Copying a profile is what decision 0032 avoids.
- Whether the neutral part of the probe, naming a surface's kind and its pack before any pack is chosen, belongs in the kernel.
- The file format of a scenario.
- Whether capture should also read what in-page code sends over WebSockets and server-sent events.
- Profiles for the platforms not yet checked against their own documentation.
