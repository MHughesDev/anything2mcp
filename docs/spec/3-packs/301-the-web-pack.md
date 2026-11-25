# 301. The web pack

The web pack lets a target be a web application: something a person uses in a browser, through pages, forms, and the requests those pages send. It was the first pack designed, and the kernel's first slices were planned around it; the first driver through the engine loop was the HTTP pack's ([decision 0067](../../decisions/0067-the-kernel-is-built-layer-by-layer-in-parallel-and-the-first-driver-is-the-http-packs.md)). It brings three modes: `ui`, which uses the page as a person would; `page_http`, which sends requests from inside the signed-in page; and `site_tools`, which calls tools a page registers for agents. Built today: partly; [section 704](../7-project/704-readiness.md) gives the detail.

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
| An application built on a browser runtime | It is a web surface. The supervisor starts its program by path and digest, and the pack drives it as it drives a browser it started. It fails if the application disables its inspector. By rule five it is started with a data directory the engine owns, never with the application's own, which is another program's store; an application that will not take one is refused, and whether that may ever change is the owner's question in [decision 0211](../../decisions/0211-whether-the-engine-may-ever-act-in-a-session-the-person-already-has.md) |

| Refused | Why |
| --- | --- |
| The user's everyday browser profile, or any browser process the engine did not start | Rule five. The engine never reads, copies, or drives another program's profile or cookie store ([section 215](../2-kernel/215-sessions-and-the-session-keeper.md)) |
| Hiding that it is automation, changing its fingerprint, answering a challenge | Rule five. A block is the named stop `edge_blocked` |
| Clicking by pixel or position, and XPath | Nothing improvises. Elements are found by role and name |
| A navigation or a request to an origin the target did not declare | Section [502](../5-security/502-defending-against-a-manipulated-agent.md): no input changes where a request goes |
| An unrecognized page, dialog, control, or platform | Rule seven. Each is a stop, never a guess |
| Pages that cannot be driven over the DevTools protocol, such as a browser's legacy compatibility mode | Unsupported tier. It would need a different back end |

## Modes

| Mode | Access kind | How it reaches the system | Needs | Best for | Main risk |
| --- | --- | --- | --- | --- | --- |
| `ui` | Interface | The browser engine acts on elements by role, label, text, or test id; no pixel clicks | A browser session | Writes that must pass the application's own validation; applications with no usable traffic | Layout and label changes |
| `page_http` | Request | Requests sent from inside the signed-in page with the page's own session, cookies, and headers | A browser session | Fast reads and proven writes without driving the interface | Front-end logic is skipped |
| `site_tools` | Request | Tools the site registers in the browser (WebMCP), called with typed arguments. As built: by name and the digest of the tool's description and schema, from an acting tab, as a write that cannot be taken back ([decision 0359](../../decisions/0359-the-site-tools-mode-calls-a-tool-a-page-registers-by-its-name-and-digest-as-a-write-that-cannot-be-taken-back.md)) | A browser session | Sites that offer them | An early standard, offered by few sites |

The modes are those of [decision 0005](../../decisions/0005-seven-modes-behind-one-driver-interface.md), so the pack needs no new mode record. Each mode's place in the default order of bindings is its `rank` in the pack's manifest, as [section 201](../2-kernel/201-access-kinds-and-modes.md) describes; the cheapest-first rule puts the interface last. An operation that began as `ui` steps gains a `page_http` or `http` binding safely through shadow and promotion (below).

`site_tools` belongs here and not to the MCP pack, because it needs a tab that can act and only the web pack has one ([decision 0062](../../decisions/0062-words-that-arrive-with-packs-are-registered-names-and-packs-share-data-never-code.md)). It shares ideas with `mcp`, a pinned list of tools and a typed call, and no code.

## What it registers

| Registration | Content |
| --- | --- |
| Surface kinds | `web` |
| Modes | `ui`, access kind interface; `page_http` and `site_tools`, access kind request |
| Step kinds | `goto`, `wait`, `locate`, `set`, `invoke`, `read`, `handle`, `upload`, `download`, `observe` for `ui`; `goto` and `request` for `page_http`, where `request` also checks the answer and gives its values, so no unchecked answer is held between steps ([decision 0190](../../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md)), and, for a write, reads the page's anti-forgery token inside the page in the same call that sends it, so the token never leaves the page and there is no `token` step ([decision 0206](../../decisions/0206-the-page-http-mode-writes-by-put-with-the-pages-own-token-in-an-acting-tab.md)); `call_tool` for `site_tools`. Each a checked name within its mode |
| Drivers | One per mode, sharing one `Browsers` |
| File kinds | Owns `screens/` for surfaces of kind `web` ([file formats](../../reference/file-formats.md#screensyaml)), and `page_requests/`, the `page_http` mode's entries, in the form of the endpoint catalog the HTTP pack owns ([section 303](303-the-http-pack.md)) with the page that makes each; the drafter lets a pack draft only its own kinds, so the two share one catalog only once that changes ([decision 0190](../../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md)) |
| Binding fields | `ui`: `screen`, `verb`, `fields`, `action`. `page_http`: `endpoint`; each placeholder takes the input of its name; an `update` also names `body`, a map from each body field its entry declares to an expression, and `read_back`, a read entry; `path_params` and `query` are not built ([decision 0206](../../decisions/0206-the-page-http-mode-writes-by-put-with-the-pages-own-token-in-an-acting-tab.md)). `site_tools`: `screen`, `tool`, `digest`, `args`, `result` ([decision 0359](../../decisions/0359-the-site-tools-mode-calls-a-tool-a-page-registers-by-its-name-and-digest-as-a-write-that-cannot-be-taken-back.md)) |
| Control names | Every control in the widget driver table below, as checked names |
| Finding kinds | `surface`, `sign_in_method`, `screen`, `field`, `action`, `exchange`, `declared_tool`. It produces `endpoint` findings, a kind the HTTP pack owns |
| Discoverers | Screen matching, in `a2m-pack-web-ui`; six in `a2m-pack-web-probe` (below) |
| Describer | `web_describer` |
| Auth providers | `form_login`, `window_login`, `sso_oidc`, `sso_saml` |
| Lint rules, drafting rules | Below |

`screens/` is also registered by the desktop pack, for surfaces of kind `desktop` ([section 305](305-the-desktop-pack.md)). Registration is keyed by file kind and surface kind, so the two do not collide.

If the HTTP pack is not enabled, the probe's endpoint inference is not available and says why. The pack compiles its `page_requests/` entries into its own step payload; no request-template type is shared between packs.

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

`ui` lowers each verb into `goto`, `wait`, `locate`, `set`, `invoke`, `read`, `handle`, `upload`, `download`, and `observe`. `page_http` lowers into `goto`, `token`, and `request`; as built, a read is `goto` and `request`, and an `update` is `goto` in an acting tab and `request`, a PUT marked as the commit point, read back by `goto` and `request` of a read entry in the observing tab; every other write is refused ([decision 0206](../../decisions/0206-the-page-http-mode-writes-by-put-with-the-pages-own-token-in-an-acting-tab.md)). A `page_http` `goto` the edge answers stops as `edge_blocked`, as a `ui` one does ([decision 0373](../../decisions/0373-the-page-http-write-stops-at-the-edge-and-at-an-ended-session-before-it-sends.md)). `site_tools` lowers into `goto` in an acting tab, `wait`, and `call_tool`, the commit point. Each lowering compiles what it reads, its screens and, for `page_http`, the endpoint entries the compiler hands it as validated data, into its own opaque step payload, and marks the steps that commit and the steps that are lookups. An action with `commits: true` is a commit point; the compiler counts those after lowering ([section 206](../2-kernel/206-the-compiler-and-the-compiled-target.md)). As built, `ui` declares the step kinds `goto`, `wait`, `read`, `set`, and `invoke`: each of the last three finds its own element and checks its fingerprint, and an `invoke` that commits is not done until the page shows the action's `confirm` or a declared variant, so a `ui` binding's confirm list is empty ([decision 0112](../../decisions/0112-the-ui-driver-as-built-five-step-kinds-and-a-commit-that-waits-for-its-confirmation.md)).

## The driver, part by part

| Mode | Prepare | Act | Confirm | Read back |
| --- | --- | --- | --- | --- |
| `ui` | An observing tab in the session's profile, with the target's fixed environment, for every read and every read-back in every posture; beside it an acting tab only for the steps of a write up to its commit point, which the lowering marks on each `goto` ([decision 0195](../../decisions/0195-a-read-never-runs-where-the-page-may-write-and-a-page-keeps-only-its-structure.md)) | `goto`, `locate`, `set` through the widget driver, `invoke` | The action's declared `confirm`, often an `observe`d response | The field again after the screen is reached afresh, or the save response with `prefer: response` |
| `page_http` | An observing tab in the session's profile, for a read in every posture; an acting tab in the same profile only for a write's own `goto` and `request`, after the intent, in a posture that may act ([decision 0190](../../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md), [decision 0206](../../decisions/0206-the-page-http-mode-writes-by-put-with-the-pages-own-token-in-an-acting-tab.md)) | `goto` the page that makes the request, which waits for its load; `request` sent from inside the page with GET alone for a read, or with PUT and the page's own token, read from where the entry says, for an `update` | Status and response schema, inside `request`, before any value is returned | The endpoint the binding names |
| `site_tools` | An acting tab, and the list of tools the page registers, compared with the binding's tool and schema digest | `call_tool` with typed arguments | The tool's typed result | The read tool the binding names, or the `ui` field. Without one the write is `irreversible`. As built there is no read-back: the page's acknowledgement is the evidence, and the record ends acted, never verified |

The rules every driver keeps are the kernel's ([section 207](../2-kernel/207-the-engine-loop.md)). For this pack they mean:

- **No retries inside a step.** A widget driver's escalation from fill to key-by-key typing to the native value setter is one strategy in one step, chosen by the control, not a retry.
- **Every failure says whether input reached the page.** The base reports each act as sent, not sent, or unknown. A crash or a disconnect after an `invoke` reached the page is unknown, and for a commit point that is `possible_commit`, never `transient`.
- **An element is valid for one page generation.** A navigation or a re-render that removes the node makes it stale, and acting on it is a failure, never a silent re-query. A read alone finds it again in the same document by the query that found it, in the one call that reads it, when that query matches exactly one element ([decision 0379](../../decisions/0379-a-read-of-an-element-the-page-drew-again-finds-it-by-the-query-that-found-it.md)).
- **An outcome can carry outputs known before the commit point,** such as a value the form shows before saving. The engine records them before the committing step runs.
- **A driver never signs in or renews.** It reports a sign of expiry through the lease and stops; the session keeper decides.
- **The fixed environment never varies:** viewport, locale, time zone, color scheme, and reduced motion are set per target. So is `refuse_quietly`, the origins a surface's pages are refused without a stop.
- **A tab stays on its surface.** A `ui` tab is opened for its surface's origin, and the browser engine holds it there. A move the page tries to another origin, a window, and a download stop the run; after a committing click each leaves the write in doubt.
- **A tab's page sends only where its tool says.** Every request of a tool's tab, from the page, its frames, and its workers, with any method, an image, a script, and a font included, goes only to the surface's origin and to the origins the operation declares in `permissions.destinations`, written with their scheme. The browser fails any other before it leaves and the run stops with `policy`, `origin_not_declared`, so the publish page can state where a web tool sends ([decision 0292](../../decisions/0292-a-web-tab-sends-only-to-its-surface-and-the-destinations-its-tool-declares.md)). A scout carries each origin its observing tab refused in the `screen` finding, and the drafted reads wait in the holding folder with a question for each, for a person to keep or remove; a surface may name origins to refuse without a stop (`refuse_quietly`), and the tab a person signs in in holds the requests a page loads with no body to its origins and the identity providers' ([decision 0335](../../decisions/0335-a-scout-carries-the-origins-a-page-tried-and-a-surface-may-refuse-origins-quietly.md)). Not held: a name the browser looks up, or a connection it opens, without a request.
- **An answer is what changed after the click.** A committing action is not clicked while the page already shows one of the screen's variants or the action's confirmation, since its answer could not be told from them ([decision 0144](../../decisions/0144-the-web-drivers-and-discoverers-after-their-safety-audit.md)).

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
- **Open shadow roots are pierced, and frames are declared.** A screen inside a frame declares its frame path: `frame`, a list of locators, outermost first. As built, a frame of the page's own origin is a scope that is entered; a frame of another origin has no document to enter and is never a scope. A closed shadow root falls back to the keyboard or the network.
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

As built ([decision 0370](../../decisions/0370-platform-profiles-are-data-in-profiles-and-the-first-one-applied-is-the-identifiers-a-platform-makes.md)): the files are in `profiles/` at the root of the repository, read and held to this schema by the `profiles` module of `a2m-pack-web-ui` and embedded in the program (the base does not see the YAML reader, so the loader is not in `a2m-pack-web-browser`). A surface of the kind `web` may name a `profile`, one of the files' names or `auto`; `web.profile_unknown` refuses another, and `web.generated_id` refuses a locator built on an identifier the profile's `reject_id_patterns` match, and `web.profile_readonly` refuses a write bound through a screen of a surface whose profile's tier is `read_only` or `unsupported`, in whatever mode names the screen (`ui` and `site_tools`), and says to bind it to a request instead; a read through such a screen compiles. The driver's `set` of a text control on a surface that names a profile commits the value as the profile's `input_commit` says, in that one way, and asks the control afterwards ([decision 0385](../../decisions/0385-the-set-step-commits-a-text-control-as-the-surfaces-platform-profile-says.md)); a surface that names none, or `auto`, keeps the rule of no profile until the probe's detection settles `auto`. Detection and `ready` are the probe's and are read and checked, not applied; the frame mode and the locator priority as an order are not applied yet.

A profile is data, compiled into `a2m-pack-web-browser` and versioned with it, because both the drivers and the probe need it. Applying a profile to locating and input is `a2m-pack-web-ui`'s work; choosing one for a surface is the probe's. Code never branches on a platform's name where a profile's data can say it.

```yaml
profile: server_postback_forms
detect:                       # detection rules: globals, page markers, scripts, headers, cookies
  any:
    - {dom: "form input[type='hidden'][name^='__'][name$='STATE']"}
    - {js: "typeof __doPostBack === 'function'"}
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
| `ready` | The platform's own readiness rule, used by the condition `{profile_ready: true}`. Optional: `server_postback_forms` states none, because its platform's own test is a call into the framework's script object, which would name the framework ([decision 0402](../../decisions/0402-the-rename-sweep-renames-every-name-and-keeps-what-is-stored.md)) |
| `locators` | The locator priority, and the identifier patterns the linter rejects (`web_generated_id`) |
| `input_commit` | How a text control's value is committed in the page: `blur` (put in, then the focus taken away, so the control says it changed), `enter` (put in, then Enter), `blur_then_wait_ready` (put in, the focus taken away, then a wait for the page to draw the form again, the value read from the control drawn anew; a page that does not is `commit_not_redrawn`), or `type_then_blur` (typed key by key, then the focus taken away). Any other word does not load |
| `frames` | Whether frames are declared or found |
| `drift_risk` | How often the platform changes its interface; it sets how often drift is checked |
| `support_tier` | `full`, `read_only`, API-preferred, or `unsupported`. A write binding through a screen of a `read_only` or `unsupported` surface does not compile (`web.profile_readonly`) |

A surface whose profile is `auto` and matches none is `unknown_platform`: the probe reports it, its tier is `unsupported` until a person names a profile, and a run on it stops.

### Widget drivers

One driver per kind of control. The control names are shared with the desktop pack where the control exists in both; the code is not ([section 305](305-the-desktop-pack.md)).

| Control | Set | Read back |
| --- | --- | --- |
| `text`, `textarea` | Fill; escalate to key-by-key typing, then the native value setter with events | Input value |
| `number`, `currency`, `measurement` | As text, formatted by the field type | Input value, normalized |
| `select` | Choose by stored code | Selected code |
| `combobox`, `autocomplete` | Open, type to filter, choose the option whose code or exact label matches. As built: the label is typed, the option of exactly that label is waited for (up to five seconds, for a page that loads its options late) and pressed ([decision 0351](../../decisions/0351-the-remaining-widget-drivers-built-from-what-a-page-says-of-its-controls-a-staged-file-a-grid-cell-by-its-rows-key.md)) | Selected code through the field's `options`, or the label when it has none |
| `date`, `datetime`, `time` | Type in the field's declared format; fall back to the picker. As built: the text rule, in the form the browser's field takes (`2026-12-01`, `2026-12-01T09:30`, `17:30`); a field that takes no typing is a stop, and the picker is not built ([decision 0350](../../decisions/0350-the-phase-1-cases-of-the-interface-driver-frames-answered-questions-native-dialogs-two-more-controls-and-what-the-grid-still-needs.md), [decision 0351](../../decisions/0351-the-remaining-widget-drivers-built-from-what-a-page-says-of-its-controls-a-staged-file-a-grid-cell-by-its-rows-key.md)) | Value; normalizing it to the field's time-zone rule is not built |
| `checkbox`, `toggle` | Set to the target state only if different. A switch a page draws is one that says `aria-checked` | Live checked state, never the attribute, except for a switch a page draws, which has only its `aria-checked` |
| `radio` | Choose by stored code: the field's `options` give the label of each code's radio button inside the group its locators find; a code the options do not list is refused before anything is sent | Checked option, as its code |
| `slider` | Keyboard steps to the value. As built: Right or Left, one key at a time, reading after each, at most 500; a step that does not move it, or passes the value, is a stop | Current value: the input's, or `aria-valuenow` |
| `file` | Attach the file to the input directly. As built: a name in the staging folder, `staging/` of the pack's profile root, never a path; a name that is not one plain name or not a file of the folder is refused before anything is sent (`file_not_staged`) | File name shown |
| `rich_text` | Editor-specific command, or paste as plain text. As built: the text rule, into an editable element | Editor text; normalizing it is not built |
| `grid` | Address a cell by row key and column id; scroll or filter to find the row. As built: the field names its `column`, the binding names the row's key in `rows`; the cell is pressed, typed into, and ended with Enter, in a row the page holds. A finder, and scrolling, are not built | Cell value, from the row the page holds; the network response preferred, and the only way to a row the page does not hold |
| `tree` | Expand along a path of labels. As built: the path is one text of labels joined by `/`; Right opens a closed item, Enter selects the last | Selected path |
| `tabs`, `accordion` | Activate by name. As built: the field's `options` give each code's label; the choice is pressed unless its `aria-selected` or `aria-expanded` is already `true` | Active choice, as its code |
| `dialog` | Declared as expected; confirm or dismiss by named action. As built: a dialog in the page is a variant with the handling `confirm` or `dismiss` and the action it is `with`; a dialog of the browser is declared by the action that opens it (`dialog: {says, answer}`), and one that says anything else stops the run | Closed state |
| `toast` | Observed only | Text, used as a confirmation |

A password input's value is never read, whatever a step asks.

## Edge cases

Each common behavior of web applications has one built-in handling rule, so operation files declare the behavior instead of coding around it. Several of these rules are the kernel's and hold for every kind of system; they are listed here because web applications are where each was found.

| Behavior | Built-in handling |
| --- | --- |
| Pagination: pages, cursors, "load more", infinite scroll | The `list` verb with a pager per style; results paged by cursor to the caller |
| Long-running jobs inside the application, such as exports and reports | `call`, then poll a declared status until done or a time limit. As built in the `ui` mode: the action's `confirm` is waited for within the step's time ([decision 0355](../../decisions/0355-the-web-pack-handles-the-edge-a-fixed-language-and-the-pages-of-cases-22-to-27.md)) |
| File upload and download | Declared on the step. Files pass through a scoped staging folder, given as a checked path, never through the agent's context. As built: an upload by the name of a staged file, a download no step declares is refused and stops the run, and one an action declares (`download:` with its most bytes) is written into `downloads/` of the profile under a name the browser makes ([decision 0355](../../decisions/0355-the-web-pack-handles-the-edge-a-fixed-language-and-the-pages-of-cases-22-to-27.md)), moved in from a writing folder beside it only once whole ([decision 0419](../../decisions/0419-a-declared-download-is-written-beside-the-downloads-folder-and-moved-in-once-whole.md)) |
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
- **Pipelines across targets.** Not built yet. A pipeline's step names an operation by its place in its own target's compiled target, so a pipeline calls the operations of one target only. The design, when it is built, is that a pipeline reads from one target and writes to another, with each target's session and policy applied separately.
- **Load on demand.** Only modules in use are loaded, so a large target does not slow the server.

## Sign-in in a browser

The rules for sessions are the kernel's ([section 215](../2-kernel/215-sessions-and-the-session-keeper.md)): one live session per identity, one renewer, and the person's own account used with the person's own rights. The web pack supplies what needs a browser.

| The pack supplies | How |
| --- | --- |
| `window_login`, `sso_oidc`, `sso_saml` | The person signs in themself, in a visible window of the engine's own browser that names the target and the identity: on the application's own pages, or through an identity provider's declared origins. The engine fills and reads nothing and waits for a declared sign. Built ([decision 0254](../../decisions/0254-a-person-signs-in-in-a-window-of-the-engines-own-browser-and-the-web-pack-stops-on-every-declared-sign-of-expiry.md)). Signing in silently through the device's account is not built (hypothesis H-05, brief 30) |
| `form_login` | A sign-in page with a stored credential, tried once. A refused credential is a stop, never a second try. Built ([decision 0245](../../decisions/0245-a-web-application-is-signed-in-to-through-its-own-sign-in-page-with-a-credential-stored-for-the-person.md)): the entry names the sign-in screen and its parts; success, a refused credential, a one-time code asked, and a block are each a declared sign of that screen, and a page with none is a stop. The code is asked of the person each time; only a published version's addresses take the stored credential |
| The browser profile as the session's live state | A data directory owned by the engine, one per session, on the session's home runner, given to the base as a checked path ([decision 0031](../../decisions/0031-the-browser-always-runs-with-a-profile-the-engine-owns.md)). Every tab is in an in-memory context of that browser, so no cookie and no page storage is written to the directory ([decision 0245](../../decisions/0245-a-web-application-is-signed-in-to-through-its-own-sign-in-page-with-a-credential-stored-for-the-person.md)) |
| Saved state | The cookies the browser holds for the application's origins, exported after a sign-in, each value a secret part of the session's material, sealed for its person and held by the keeper; placed into the session's browser before a run's first tab. Page storage is not exported yet |
| Silent renewal | The keeper's first rung: loading the application in a background tab so the application's own silent renewal runs |
| Signs of expiry | A screen the target declares in `expiry_signals` as `{screen: <name>}`: a `ui` wait that finds it in place of its own screen stops with `session_expired`; in `page_http`, a page answered by that screen, or a request answered with a status the target declares, stops the same way, before anything is sent from the page. The pack reports them; it acts on none |

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

Screen matching, in `a2m-pack-web-ui`, sets a stored snapshot finding against the target's screen models. It reads findings, not the page. As built, the first half of it is in `web_snapshot`: pages of one discovery whose paths differ in exactly one segment and whose snapshots give the same named tables, headings, and labelled values are given as one `screen` finding whose path holds that segment as a placeholder, naming the pages it was seen on ([decision 0187](../../decisions/0187-the-web-pack-matches-a-screen-by-its-address-and-reads-by-key.md)). The describer shows an agent snapshots in the text form of the [steps reference](../../reference/steps-snapshots-and-diffs.md#snapshots), captures as exchanges by step, and endpoints as method, template, and shapes, with every string from the target marked untrusted. What a finding keeps of an address and an answer is structure alone: a segment of a path that is not a plain word is the placeholder `{value}`, an entry of a query with no name is not kept and is said to have been there, the names of an object that read as a dictionary's keys are withheld, and the headers a page's script set are kept by name ([decision 0195](../../decisions/0195-a-read-never-runs-where-the-page-may-write-and-a-page-keeps-only-its-structure.md)).

Hitting a bound is not a failure. It is a limitation on the findings, `truncated` or `time_limit`, and the report names it. Two scouts of the same application in the same state give the same findings.

### Discovery cannot write

This is held in four places, and each would hold without the others.

| Where | How |
| --- | --- |
| The type | The probe is constructed with an `Observer`. It opens observing and watching tabs only; neither has an input method, and there is no way to get an acting tab from one. A check in CI refuses the names of the acting types in the probe's source |
| The browser | An observing tab fails every request whose method is not GET, HEAD, or OPTIONS before it leaves the browser, from the page, its workers, and its frames, unless a person marked that request safe. As built, nothing can mark one: every caller hands the tab an empty list. How a person marks one is designed below, under "A page that reads by a method that may write" |
| Navigation | A link is followed only when it is a plain link to a declared surface, it is not a form, and its address does not look like an action by the pack's list of such words (sign-out, delete, remove, approve, confirm, send, pay, and the like). A refused link is counted, never silently dropped. Every address discovery visits passes the same check, whatever named it: a link, a focus path, or the surface itself. The drivers hold a key's value and a page request's path to the same list |
| The kernel | Discovery runs as a job on a runner, inside the engine's sandbox, with a lease the keeper marked for discovery. A discoverer is given only what the sandbox port offers: no driver, no ledger token, no way to act |

In an observing tab:

| What could change the system | What happens |
| --- | --- |
| A request whose method is not GET, HEAD, or OPTIONS | Intercepted and failed before it leaves the browser, unless it matches one of the tab's safe requests in its method, origin, path template, and body ([decision 0394](../../decisions/0394-an-asserted-read-of-a-web-surface-is-a-page-request-matched-by-one-shared-shape-and-shown-at-publish.md)), which only a person sets. Each one is reported as a limitation on the finding |
| A request of any method, GET, HEAD, and OPTIONS among them, that carries a header overriding its method (`X-HTTP-Method-Override`, `X-HTTP-Method`, or `X-Method-Override`) | Failed before it leaves the browser and reported as above, whatever the header names: a system that honours the header reads the request as the method it names ([decision 0270](../../decisions/0270-the-http-pack-sends-an-asserted-read-only-from-a-published-version-and-bounds-what-it-leaves-running.md)). It matches no safe request ([decision 0407](../../decisions/0407-an-observing-tab-reads-a-safe-requests-body-from-the-interceptions-parts-and-refuses-every-reading-two-systems-could-differ-on.md)) |
| A request of any method whose address's query carries an entry overriding its method: `_method`, compared without regard to letter case and after percent-decoding, in any position, repeated, with any value or none, `GET` among the values | Failed and reported as above, whatever it names: a server may take the method from the query, and a `GET` to `/items/7?_method=DELETE` may delete ([decision 0270](../../decisions/0270-the-http-pack-sends-an-asserted-read-only-from-a-published-version-and-bounds-what-it-leaves-running.md)). A request marked safe has no query, so it is failed already |
| A request from a worker the tab starts | A dedicated worker's requests pass through its page's interception, which is on before the page runs; any other child, a frame in another process or a service worker, takes its own before it runs or is not let run ([decision 0111](../../decisions/0111-an-observing-tabs-lock-as-built-and-what-the-browser-was-measured-to-do.md), [decision 0143](../../decisions/0143-the-browser-engine-after-its-safety-audit.md)) |
| A WebSocket | Refused at creation, and reported as a limitation |
| A request to an origin the tab was not opened for | Failed before it leaves the browser, whatever its method, and reported as a limitation (decision 0292) |
| A beacon, a form submitted with POST | Failed by the same interception |
| A download, a window the page opens | Refused in the browser: no file is written, and the window is closed before it fetches anything. Each is reported as a limitation |
| A permission, a file chooser | Every permission refused in the browser, for every origin, with no prompt; a file chooser opens nothing |
| Navigation | Only to an address the caller passes, on an origin the tab was opened for, or a plain link as above. A move the page starts itself, with any method, is ended before its request leaves the browser |

### The watch tab and the explore tab

Exploring and capturing a person's own use are two different things, and the pack keeps them apart.

| | Exploring | Capturing a person's use |
| --- | --- | --- |
| Tab | An observing tab | A watching tab, in a visible window |
| Who acts | Nobody. Nothing can press, type, or submit | The person, who may save. That save is the person's act |
| Interception | Every request that could write is failed | None by a request's method: the person is entitled to save. Documents and what a page loads are held to the surface's origins, as in the sign-in window, and nothing the engine sends goes through this tab |
| Started by | The scout, `discover.run`, or an authoring action | Only the person whose use it records, with `capture.start`; they stop it with `capture.stop` and are told what is kept ([section 213](../2-kernel/213-effects-hazards-and-consent.md)) |

The scout never opens a watching tab.

A capture of a person's use runs in the watching tab, in the visible window the sign-in uses, held to the origins the surface declares. The engine sends nothing there: it listens to the exchanges with the surface's origin and takes a snapshot, by structure, of each screen the person visits. Of a request it keeps the method, the path by structure, the names of its query and of a structured body's fields, and the shape of the answer, never a value. The watching tab is built in the base ([decision 0411](../../decisions/0411-the-watching-tab-records-from-the-browsers-network-events-and-keeps-each-screen-without-its-values.md)): it hears each request from the browser's network events, keeps a body's media type and its fields' names and kinds, and keeps each screen's snapshot without its values. The capture job and the discoverer over it are not built. The first capture was once scoped to the observing tab ([decision 0308](../../decisions/0308-the-first-capture-records-reads-only-in-the-engines-own-browser-and-drafts-to-the-holding-folder.md), point 1); [decision 0392](../../decisions/0392-a-page-that-reads-by-a-method-that-may-write-is-learned-from-a-persons-watched-use-and-read-through-an-asserted-request.md) puts it here, because an observing tab shows a capture only what a scout already sees.

### A page that reads by a method that may write

Many pages ask for their own data with `POST`: a search form, a query endpoint that takes its question in a body, a page that posts its form back to draw a grid's next page. An observing tab fails that request unless a person asserted it: without that, a `ui` read stops as `policy`, `page_write_refused`, and a scout sees the screen empty. The design ([decision 0392](../../decisions/0392-a-page-that-reads-by-a-method-that-may-write-is-learned-from-a-persons-watched-use-and-read-through-an-asserted-request.md), [decision 0394](../../decisions/0394-an-asserted-read-of-a-web-surface-is-a-page-request-matched-by-one-shared-shape-and-shown-at-publish.md)). Built: the assertion, the letting through, and the showing. Not built: the learning:

| Step | What happens |
| --- | --- |
| Learned | In a capture, where the request passes because the person made it. Seen at least twice with one shape and a structured answer, it is drafted as an endpoint in the holding folder with its body's structure as `request_schema` and an open question in place of its effect. The question says that the capture saw no value and that seeing no change is not evidence. No operation is drafted from it |
| Asserted | A person answers by writing `effect: read` and `read_asserted: true` on the page request in `page_requests/`, with the word and the meaning of the HTTP pack's key ([section 303](303-the-http-pack.md)). It compiles only on a method other than `GET`, with no body or a `request_schema` closed by `additionalProperties: false`, each field a `type` and, for a string, an `enum`. A drafter never writes it, and no mode sends it. `page_requests/` is the one place that says it (rule 8) |
| Let through | The drivers hand an observing tab the asserted reads of the compiled target they run: in a live run of a published version, and in a try or a shadow of one in an environment of kind `test`. Never under the drafts, and never in discovery. A request the tab fails whose method and path an asserted read names stops with a next step that names the page request, and, under the drafts, says to publish it. A request matches by method, origin, the entry's path as a template, segment for segment, and its body, which must validate against the entry's closed `request_schema`, any `enum` included |
| Shown | The publish page names each asserted read, after where the surfaces lead and the sign-ins and before the tools, with its method, its path, and its body's fields with their `enum`, and says that the person's word is the only evidence it changes nothing ([decision 0426](../../decisions/0426-the-publish-page-names-each-asserted-read-after-the-sign-ins-from-what-each-file-kind-says-it-asserts.md)) |
| Not covered | Where a read and a write to one address differ only in a long text of the body, no `enum` can tell them apart. Pinning that text by digest is a later package |

### Capture

A capture follows the rules of observation in [section 401](../4-building-tools/401-overlaying-tools-on-an-existing-system.md): one scenario per operation, every exchange attributed to the scenario step in force, the target's fixed environment, shapes by default and values only when a person asks and policy allows, and bounds that are counted. Capturing inside the engine's browser needs no system proxy and no installed certificate. Cookie, authorization, and set-cookie headers are removed by name before anything leaves the base, and every value the redactor knows is replaced. A capture that outlives its bound is stopped and reports what it dropped. Capture is a capability of the provider contract, not a part of the web pack: a pack that offers it declares the surface kinds it captures under `points.traffic_capture` in its manifest, a pack that declares none is asked nothing, and what a capture yields has no field for a value and passes the contract's check, which withholds every name, segment, or request that holds a secret the redactor knows or is not of its structural form. The web probe pack offers it for `web` surfaces from the observing tab, and its endpoint inference reads the page's own requests through the contract. A capture of values, which this section allows when a person asks, has no form in the contract yet ([decision 0391](../../decisions/0391-traffic-capture-is-a-capability-of-the-provider-contract-that-yields-structure-only.md)).

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
| `screen`, from a snapshot | For each table the page names and the snapshot can tell apart: a screen model with `reach` the observed path, `ready` the table showing, and `rows` located by the table's role and name and read by its headings, each column `untrusted_text`; and a `list` read over it. For the values the page labels (a definition labelled by a term of the same name): a screen whose fields locate each by the role `definition` and its label, and a `get` read of them as `untrusted_text`. A finding the discoverer matched from several pages has a placeholder in its path, which is each read's input and key | What the screen is for; its name beyond the mechanical one; two tables of one name; a label two terms share, apart from the read of the others |
| `field`, from a snapshot | A field: `control` from the role by a fixed table (textbox to `text`, combobox to `combobox`, checkbox to `checkbox`); `locate` as role and name, then label; `fingerprint` from the nearest heading or section | Its field type. A field type is meaning, so the draft does not compile until it is settled |
| `action`, from a snapshot | Nothing | Every action, and whether it commits |
| A tool a page registers, marked read-only by the page | A `site_tools` binding for one read operation | Tools not marked read-only; whether the page's marking is believed |

As built, only the `screen` row is drafted, for tables and labelled values; fields that are set, actions, and registered tools are not ([decision 0183](../../decisions/0183-the-web-pack-drafts-a-read-of-each-table-a-snapshot-names.md), [decision 0187](../../decisions/0187-the-web-pack-matches-a-screen-by-its-address-and-reads-by-key.md)). Endpoint findings are drafted by the HTTP pack's rule. An `exchange` a page made by itself drafts a `page_requests/` entry and a `page_http` read only when the driver can make the request again exactly, with no query and no header the page's script set; otherwise the entry holds an open question that names them, by name alone, and no read is drafted. Every question that quotes a page's words keeps them apart from the product's sentence ([decision 0195](../../decisions/0195-a-read-never-runs-where-the-page-may-write-and-a-page-keeps-only-its-structure.md)). Whether drafts by rule save work is hypothesis H-31.

**A column, a value, or a response value named as a secret is never drafted as an output.** The drafter leaves out of a drafted read each table column whose heading, each labelled value whose label, and each value of a page's request's answer whose name the compiler takes for a secret's, and the last sentence of the drafted description tells the names left out. An address placeholder so named drafts nothing for that address. A field that is set is never drafted, so a password field is never an input ([decision 0302](../../decisions/0302-a-field-named-as-a-secret-is-never-drafted-as-an-input-or-an-output.md)).

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
| `policy` | `origin_not_declared` | A request of a tool's page, a frame, or a worker to an origin that is neither its surface's nor one its operation declares, failed in the browser (decision 0292) |
| `busy` | `browser_unavailable` | The browser is not installed |
| `busy` | `memory_short` | The engine's browsers hold their memory ceiling, so no browser work is started; the failure carries a retry time ([decision 0425](../../decisions/0425-a-browsers-memory-is-measured-from-its-own-list-of-its-processes-and-past-a-ceiling-no-new-browser-work-starts.md)) |
| `transient` | `timeout`, `connection_lost` | Before anything was sent. Also a browser that was started and did not answer before the launch deadline: it is closed, and a slow start on a cold machine passes ([decision 0134](../../decisions/0134-one-browser-starts-at-a-time-and-a-silent-start-is-slow-not-foreign.md)) |

`locator_ambiguous`, `unknown_platform`, and `browser_unavailable` are proposed codes, settled as the crates are built. In discovery, `unknown_platform` is not a stop: the finding says so. `edge_blocked` stops discovery of that surface, and `needs_login` is reported per surface while the scout carries on with the others.

## The test stand

The test application ([test application plan](../../plan/test-application.md)) is the pack's test stand: an imaginary order-entry application, the `example` target, with a control interface that reports what it stored and how many times, and faults that can be switched on. Its cases 1 to 27 are the web pack's. A page that registers tools, for `site_tools`, is `/cases/site-tools` (package 8.2a).

| Cases | What they prove for this pack |
| --- | --- |
| 6 to 12 | Locating without generated identifiers, widget escalation, in-page navigation, grids by key, custom controls (built: the page `/cases/controls`, [decision 0351](../../decisions/0351-the-remaining-widget-drivers-built-from-what-a-page-says-of-its-controls-a-staged-file-a-grid-cell-by-its-rows-key.md)), declared dialogs, frames and shadow trees |
| 13 to 15 | The auth providers, sign-in detection, the keeper's probes, exclusive sessions |
| 16 to 21 | Normalization, `possible_commit` after a crash, record matchings, dependent records, no duplicate send, `changed_since_preview` |
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
| 3 | Each record matching | `natural` and `composite`: `reach` built from the key. `stamped`: write the stamp field, then find the record through a list screen. `find`: a search screen, with the declared `settle`. `none`: the write is `irreversible` |
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
- Whether the shared parts of `screens/` should become the interface access kind's, in the kernel, once the desktop pack exists (hypothesis H-37).
- How the `page_http` lowering and the probe are held to the version of the `endpoints/` and `endpoint` finding schemas they were written for, when the HTTP pack changes them.
- The tool registry a page offers is an early proposal (brief 24); its shape is settled when the standard is. [Research note 25](../../research/25-webmcp-standard-status.md) found that such tools can be discovered and called through the browser's debugging protocol in a current browser build, and that the standard is still a community group draft.
- Whether a validation message is always a declared variant, or a profile can name a generic one.
- How a GET that a page treats as an action is recognized. A word list is a start, not an answer; it is settled in Phase 4.
- Whether observing and acting tabs in one browser are safe enough together, or an observing scout should use its own browser. Copying a profile is what decision 0032 avoids.
- Whether the neutral part of the probe, naming a surface's kind and its pack before any pack is chosen, belongs in the kernel.
- The file format of a scenario.
- Whether capture should also read what in-page code sends over WebSockets and server-sent events.
- Profiles for the platforms not yet checked against their own documentation.
