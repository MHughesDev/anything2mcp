# `a2m-pack-web-ui`

The web pack's drivers and the knowledge they carry: the `ui`, `page_http`, and `site_tools` drivers, the widget drivers, platform profiles, screen models as a file kind, the auth providers that need a browser, and the drafting rules for observed interfaces. It is built on `a2m-pack-web-browser` and is called back by the engine and the session keeper.

Built so far: the `ui` driver for reads and verified writes, with an observing tab for a run that may only look, the `screens/` file kind, the `ui` lowering, the drafting rules from a snapshot's tables and labelled values, by key over a matched screen, and the read half of `page_http` with the `page_requests/` file kind and its drafting rules ([decision 0190](../../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md)) ([decision 0183](../../decisions/0183-the-web-pack-drafts-a-read-of-each-table-a-snapshot-names.md), [decision 0187](../../decisions/0187-the-web-pack-matches-a-screen-by-its-address-and-reads-by-key.md)). Everything else on this page is the design of what follows, and the build order says which is which.

## Facts

| | |
| --- | --- |
| Side | Pack: web |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5, and `a2m-pack-web-browser` |
| Depends on | `a2m-core` (with `Plan` and `Step`), `a2m-files` (file-kind, lowering, and lint registration), `a2m-auth` (`SessionLease`, `CredentialSink`), `a2m-provider` (`Driver`, `Pack`, `PackContext`, registration), `a2m-telemetry` (`Acting`), `a2m-pack-web-browser`. Outside: `tokio`, `serde`, `serde_json`. `a2m-expr` is not used yet: formatting a date or a quantity for a control is not built |
| Built in | Slice 1.1 (`ui` reads one field), 1.3 (registration with the compiler), 1.6 (read verbs, widget reads), 2.3 and 2.6 (auth providers), 3.3 onward (writes), 4.2 (`page_http`), 8.2 (`site_tools`), 8.4 (profiles, remaining widgets), 8.5 (edge cases) |
| Carries out | Spec sections [201](../../spec/2-kernel/201-archetypes-and-modes.md) (the web modes), [301](../../spec/3-packs/301-the-web-pack.md), [301](../../spec/3-packs/301-the-web-pack.md) (the interface rows), [215](../../spec/2-kernel/215-sessions-and-the-session-keeper.md) and [215](../../spec/2-kernel/215-sessions-and-the-session-keeper.md) (browser sign-in), [201](../../spec/2-kernel/201-archetypes-and-modes.md) (interface and request), [402](../../spec/4-building-tools/402-the-scout-and-who-does-what.md) (drafting from an observed interface); the technical reference's [widget drivers](../../spec/3-packs/301-the-web-pack.md#widget-drivers) and [platform profile schema](../../spec/3-packs/301-the-web-pack.md#platform-profiles) |

## Owns

- The drivers for `ui`, `page_http`, and `site_tools`, one `Driver` each, sharing one `Browsers`.
- Turning a compiled screen into queries and waits for the base: locator order, fingerprints, frame paths, variants.
- The widget drivers: how each control is set, escalated, and read back.
- Applying platform profiles: locator priority, rejected identifier patterns, input commit, frame mode, and the support tier's limit on writes. The profiles themselves are data in `a2m-pack-web-browser`, because the probe needs their detection rules too.
- Rules that recognize, in a page or a response, an edge block, a sign-in page, or an expiry signal. It reports them; it acts on none.
- The auth providers that need a browser: `sso_oidc` and `sso_saml` in an engine-owned profile, and `form_login`.
- The `screens/` schema for web surfaces, the binding fields of its modes, its lowerings, lint rules, and drafting rules.

It must never: decide whether something may happen; sign in or renew outside the keeper's call; write to the ledger; retry inside `act`; branch on a platform name in code where a profile's data can say it; treat an unrecognized page, dialog, or control as anything but a stop.

## What it registers

As built. The manifest is `src/anything2mcp-provider.toml`; what is not built is not declared.

| Registration | Content |
| --- | --- |
| Provider | `web`, a pack |
| Modes | `ui`, archetype interface, rank 60, four reads and one write at a time on a session. `page_http`, archetype request, rank 40, verb `read`, steps `goto` and `request`, neither of which saves ([decision 0190](../../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md)). `site_tools` is not built |
| Surface kinds | `web`, with the address key `origin` and the fields `viewport` (`width`, `height`), `color_scheme` (`light` or `dark`), and `reduced_motion`. `profile` is not accepted yet |
| Step kinds | `ui`: `goto`, `wait`, `read`, `set`, `invoke` ([decision 0112](../../decisions/0112-the-ui-driver-as-built-five-step-kinds-and-a-commit-that-waits-for-its-confirmation.md)). `page_http`: `goto`, which reaches the page that makes the request and waits for its load, and `request`, which makes the read, checks its status, media type, and shape, and gives its values. `handle`, `upload`, `download`, `observe`, `token`, and `site_tools`'s kind are not built |
| Drivers | `UiDriver` and `PageHttpDriver`, over the `Browsers` of the cell the product crate hands the pack |
| File kinds | Owns `screens/` for surfaces of kind `web` ([file formats](../../reference/file-formats.md#screensyaml), [decision 0113](../../decisions/0113-a-screens-fingerprint-is-a-section-absent-is-a-variant-and-a-table-is-read-by-its-headings.md)), and `page_requests/` (`page_request`) for them: a part of an endpoint entry (`endpoint`, `surface`, `method: GET`, `path`, `effect: read`, `response_schema`) and `page`, the page that makes it. It is here, not in the HTTP pack's `endpoints/`, because a rule may draft only a kind its pack owns ([decision 0190](../../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md)) |
| Binding fields | `ui`: `screen`, `fields`, `action`, and `read_back` with a `screen`. `page_http`: `endpoint`, the name of a page request; each placeholder takes the input of its name |
| Verb lowerings | `ui`: `read`, `list`, `create`, `update`, `upsert`, `submit`, below. `open`, `search`, `delete`, `upload`, and `download` are not built |
| Control names | `text`, `textarea`, `number`, `currency`, `measurement`, `select`, `checkbox`. The compiler refuses any other with `kernel.unknown_control` |
| Lint rules | Below |
| Settings | `packs.web.max_read_chars` and `packs.web.max_snapshot_elements` |
| Effects | Writes the root `profiles`; network to surfaces; sessions for `web` surfaces; a requirement that lists every path of the browser engine's `KNOWN_PLACES` |
| Drafting rules | From a `screen` finding to `screen` files and `operation` files, and from an `exchange` finding to `page_request` files and `operation` files, below |
| Discoverers, auth providers | None yet |

### Lint rules

Codes are stable once released. A lint code is the pack's name, a dot, and the rule.

| Code | Level | Rule | Built |
| --- | --- | --- | --- |
| `web.css_not_last` | Error | A `css` locator is allowed only last in a list | Yes |
| `web.css_used` | Warning | Any `css` locator | Yes |
| `web.no_fingerprint` | Error | A field, an action, or a table with no fingerprint | Yes |
| `web.generated_id` | Error | A locator matches a rejected identifier pattern of the surface's profile | No: profiles |
| `web.commit_unconfirmed` | Error | An action with `commits: true` and a confirmation that could be true before it acted | No |
| `web.variant_unhandled` | Error | A variant with no handling | The schema requires a handling |
| `web.frame_undeclared` | Error | A screen whose locators need a frame and declare none | No: frames |
| `web.profile_readonly` | Error | A write binding on a surface whose profile's tier is `read_only` or `unsupported` | No: profiles |
| `web.signed_request` | Error | A `page_http` binding on an endpoint marked as signed in the browser, used for a write before it is verified | No: the write half of `page_http` |

The file kind and the lowering refuse, under the same prefix: `locator_invalid`, `condition_invalid`, `within_and_section`, `read_not_fitting`, `column_unknown`, `reach_invalid`, `unknown_screen`, `unknown_field`, `unknown_action`, `placeholder_unbound`, `position_type`, `value_type`, `action_missing`, `read_acts`, `read_back_never_absent`, and `not_encodable`; for `page_http` and its file kind, `unknown_endpoint`, `page_http_writes`, `output_kind`, `output_not_required`, `request_path_invalid`, and `request_shape_invalid`.

### Drafting rules

As built ([decision 0183](../../decisions/0183-the-web-pack-drafts-a-read-of-each-table-a-snapshot-names.md)). The finding is the probe's, read as data: this crate does not name `a2m-pack-web-probe`, and does not declare the kind under `reads`, so that a registry with only this pack has no missing owner.

| From a finding of kind | The drafter writes | Left out, or as an open question |
| --- | --- | --- |
| `screen`, from a snapshot | For each table the page names, readable row by row, inside a part of the page the snapshot names once: `screens/{page}_{table}.yaml`, with `reach` the finding's path, `ready` the table showing, `rows` located by the table's role and name, the part of the page as the fingerprint, every heading, and one column of type `untrusted_text` for each heading, and `operations/{page}_{table}_list.yaml`. For the values the page labels, each a `definition` whose name one `term` of the page also has, once, in a part of the page named once: `screens/{page}_values.yaml`, one field each located by the role `definition` and the label, read as text, and `operations/{page}_values_get.yaml`, `{target}_{page}_get`. A placeholder in the finding's path, as screen matching gives it, is each read's input, of type `text`, and its key; `{page}` is the path's plain words ([decision 0187](../../decisions/0187-the-web-pack-matches-a-screen-by-its-address-and-reads-by-key.md)) | A table with no name; a column whose heading another shares; a page cut short, of another origin than one `web` surface, or with a path no address can hold as written. Two tables with one name: an open question in place of the locator. A label two terms share: no field for it, and an open question apart, in `screens/{page}_values_undecided.yaml` |
| `screen`, from a snapshot | For each such screen that holds no question: `operations/{page}_{table}_list.yaml`, `{target}_{page}_{table}_list`, `effect: read`, `verb: list`, no input, one list output of type `untrusted_text` for each column | An example, a test, a key |
| `field`, `action`, a form, a button | Nothing | Every field and action, and what a field means. A drafted screen with an action is refused by the drafter |
| `exchange`, a request a page made by itself (`made_by`), GET, JSON, status 200, on one `web` surface; matched by key when the discoverer joined several ([decision 0190](../../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md)) | `page_requests/{request}.yaml`, with the path, the page, and the answer's top-level names and kinds, required when every answer held them; `operations/{request}_request.yaml`, `{target}_{request}_get`, `page_http`, verb `read`, the placeholders as inputs and key, the plain values as outputs: a string `untrusted_text`, an integer `integer`, a boolean `boolean`. `{request}` is the request path's plain words, so a screen's reads, named from the page's path, never take its name | Any other method, an answer that is not JSON, another origin, a document the tab was sent to; a number with a fraction, a nested value, a value some answers left out; an example |
| A tool a page registers, marked read-only by the page | Not built: a `site_tools` binding for one read operation | Tools not marked read-only; whether the page's marking is believed |

Screen matching is done by the discoverer, which sees every page of one discovery: the drafter calls a rule with one finding. Setting a stored finding against existing screen models is not built.

## Interface

As built. The product crate makes one cell of browsers and hands a clone to this pack and to the web pack's discoverers, so that the two share one set of browsers:

```rust
// `SharedBrowsers` (Clone, Default) is the base crate's, exported again here.

/// The pack. `Pack::register` builds the browsers from its context, or
/// takes the ones the other half of the web pack built.
pub fn pack(browsers: SharedBrowsers) -> WebUiPack;
/// The one call the product crate makes.
pub fn add_to(builder: &mut RegistryBuilder, browsers: SharedBrowsers);

pub const UI_MODE: &str = "ui";
pub const PAGE_HTTP_MODE: &str = "page_http";
pub const REQUEST: StepKind;                         // `request`, of `page_http`
pub const REQUEST_KIND: FileKindName;                 // `page_request`
pub const ADDRESS_KEY: &str = "origin";
pub const PACK: &str = "web";
pub const SCREEN_KIND: FileKindName;                  // `screen`
pub const WEB_SURFACE: SurfaceKind;                   // `web`
pub const CONTROLS: &[&str];
pub const GOTO, WAIT, READ, SET, INVOKE: StepKind;
pub mod codes;                                        // every failure code, and `ALL`
```

The host's roots must hold a folder named `profiles` that may be written. The driver, the file kind, the lowering, the compiled screens, and the widget rules are private.

### The steps

| Step | Refers to | Arguments | Does |
| --- | --- | --- | --- |
| `goto` | A screen | `reach.<placeholder>` for each placeholder of its address | Builds the address from the surface's origin and the screen's path, each value one plain segment (letters, digits, `-`, `.`, `_`, `~`, not dots only); any other value is `input`, `address_value_refused`, before the tab is sent anywhere, never encoded. The made address is checked against the origin and the screen's pattern, and sends the tab there. A page that ends on another origin is `policy`, `surface_not_declared` ([decision 0187](../../decisions/0187-the-web-pack-matches-a-screen-by-its-address-and-reads-by-key.md)) |
| `wait` | A screen | None | Waits, on the base's observer, until the screen's `ready` holds or one of its variants shows. A variant `absent` is the outcome absent, for a lookup; a variant `stop` is `rejected`. In a write binding it keeps a snapshot |
| `read` | A screen | None | For each output, finds the field of its name and reads it by its control's rule, as the text the page shows; for a list, reads the screen's table by its headings |
| `set` | A screen | `value` | Finds the field and sets it by its control's rule |
| `invoke` | A screen | None | Finds the action, and does not click it when the page already shows one of the screen's variants or the action's `confirm`: its answer could not be told from them. Otherwise clicks it, and waits until the page shows the action's `confirm` or a variant. A variant is the application's answer, except that after a committing click an `absent` variant, or a variant shown together with the confirmation, leaves the write in doubt. Marked as the commit point when the action says `commits`. Keeps a snapshot |

The tab is opened for the surface's own origin, and the browser engine holds it there ([decision 0143](../../decisions/0143-the-browser-engine-after-its-safety-audit.md)): a move to another origin is ended in the browser, and no page of another origin is located, read, or acted on. What the tab refused, a move of the page, a window, or a download, is heard at the start and the end of every step and while a click's answer is awaited, and stops the run: `policy`, `surface_not_declared` for a move, `unknown_variant`, `unexpected_page` for a window or a download, and `possible_commit`, `no_confirmation` once a committing click was made ([decision 0144](../../decisions/0144-the-web-drivers-and-discoverers-after-their-safety-audit.md)). Another destination of the run is another surface, whose screens the run does not hold, so it is not one of the tab's origins.

A run against a declared test record sets fields and performs only the action marked as the commit point; a run that may only look sets and clicks nothing ([decision 0147](../../decisions/0147-the-engine-after-its-safety-audit.md)). The lowering states that only an `invoke` may save, as its action declares, and that a `goto`, `wait`, `read`, or `set` never does; no kind of step always saves, so the manifest lists no `commit_steps`. A binding's least effect is the HTTP pack's rule: a verb that reads is `read`, `submit` is `irreversible`, and any other write is `write` with a `read_back` and `irreversible` without one. A screen that declares any action describes a write, and the drafter writes none.

`read`, `set`, and `invoke` each find their element through the field's or action's locators in order, and check its fingerprint. A locator that matches several elements, and a match that fails its fingerprint, stop the step; a later locator is tried only when an earlier one matches nothing.

### Control rules

| Control | Set | Read |
| --- | --- | --- |
| `text`, `textarea`, `number`, `currency`, `measurement` | Fill; when the control does not hold the text, type it key by key; when it still does not, the element's own setter. The control is asked what it holds after each. A password input is filled once and never asked | The value, as text |
| `select` | Choose the option with the stored code | The selected code |
| `checkbox` | Bring to the state asked for | The live checked state |

A value reaches a control as text, a code, a whole number, an exact number, or a truth value. A date, a time, and a quantity shown in another unit need the field type's format, which is not built: the compiler refuses the binding with `web.value_type`.

What the kernel gives this crate:

- `Driver` has four calls. Confirm and read-back are lists of steps that `act` carries out ([steps, snapshots, and diffs](../../reference/steps-snapshots-and-diffs.md)). A step marked as a lookup may report a record as absent. Every failure says whether the request was not sent, was answered, or is unknown: a crash after an `invoke` reached the page is unknown. The engine reaches the driver through a handle that refuses a committing step without the ledger's intent token.
- An outcome can carry outputs known before the commit point, such as a value the form shows before saving; the engine records them before the committing step runs.
- `Plan` and `Step` are data in `a2m-core`. A step's payload is opaque bytes that this crate's lowering wrote and its driver reads; the kernel stores and hashes it and never decodes it.
- `page_http` reads the `page_requests/` this pack owns, written in the endpoint catalog's form, and compiles them into its own payload; no request-template type is shared. Sharing one catalog needs the drafter to let a rule draft a kind its pack reads, or the HTTP pack to draft from an `exchange` that names `made_by` ([decision 0190](../../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md)).
- `SessionLease` lets a driver report an expiry signal and reach the session's live handle, here a `ProfileKey`, without holding a credential. `inject` places a token into a `page_http` request header without returning it.

### How each mode fills the loop

| Mode | Prepare | Act | Confirm | Read back |
| --- | --- | --- | --- | --- |
| `ui` | A new acting tab in the session's browser, with the target's fixed environment; in discovery, and in the sandbox with no test record to write, an observing tab, in which the browser fails every request of the page that could change the system | `goto`, `wait`, `set` through the control's rule, `invoke` | The action's declared `confirm`, inside the committing `invoke`; the confirm list is empty | The fields again after the screen is reached afresh. `prefer: response` is not built |
| `page_http`, as built for reads ([decision 0190](../../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md)) | An observing tab in the session's profile, in every posture | `goto` the entry's `page`, which waits for its load; `request`, through the engine's `read_from_page`: GET alone, to the entry's path with each placeholder one plain segment, no header, no body, no redirect | Inside `request`: status 200, a JSON media type, the answer's shape against the entry, before any value is returned | None: it only reads. The write half needs the page's anti-forgery token placed without being read, a commit point, and a read-back |
| `site_tools` | An acting tab, and the list of tools the page registers, compared with the binding's tool and schema digest | `call_tool` with typed arguments | The tool's typed result | The read tool the binding names, or the `ui` field. Without one the write is `irreversible` |

### The entry bar

| # | Item | This crate's answer |
| --- | --- | --- |
| 1 | A driver for its verbs, no retries inside | The three drivers above. A widget driver's escalation from fill to typing to the native setter is one strategy in one step, chosen by the control, not a retry |
| 2 | A read-back rule for every write | As in the table above |
| 3 | Each identity strategy | `natural` and `composite`: `reach` built from the key; built. `stamped`: write the stamp field, then find the record through a list screen; not built. `find`: a search screen, with the declared `settle`; not built. `none`: the write is `irreversible` |
| 4 | A test stand | The test application, cases 1 to 27; `site_tools` needs a page that registers tools, which is a new case. As built, the application has cases 1 to 4: its line-item form and its tables. A page with a password, a check box, and a select is served by the test that needs it |
| 5 | A discoverer | Snapshots and capture are in `a2m-pack-web-probe`, and so is the matching of pages into one screen (decision 0187); setting a finding against existing screen models would be here |
| 6 | Failures mapped | Below |
| 7 | Effects and hazards | Starts the browser, by path and digest, through the supervisor its context scopes; reads and writes the profile root and a staging folder for files; network to the target's declared web surfaces for what the engine itself sends; holds sessions for `web` surfaces; changes a person's screen only for a sign-in window. Hazards come from operations, not from the driver |
| 8 | A record per new mode | None: these modes are from decision 0005 |

### After the kernel's second pass (decisions 0161 and 0162)

- Every step input is built with `StepInput::new` and `for_record`. The `acting` capture is made with `Capture::acting`; its bytes are the ones written before. Five runs of one read give one signature (the existing test).
- A refusal the page shows, a screen variant handled `stop`, carries the text of that variant's element through `StepFailure::with_said`, read through the browser base, which redacts every text it reads. The failure's sentence names the variant and nothing the page said.
- No write here states `acknowledged`: the package named none for this pack. `delete` is `irreversible`; a screen that declares any action counts as a write.
- No version guard of an endpoint, and no key argument, exists in this pack: `page_http` only reads.
- Keys: no step reads `StepInput::key`.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `anything2mcp` | `add_to`, or `pack`, with the `SharedBrowsers` cell it also gives the web pack's discoverers |
| `a2m-provider` | `Pack::register`, and the driver it registers |
| `a2m-engine`, through the registry | The `ui` driver, only |
| `a2m-files`, through the gathered set | The `web` surface kind, the `screens/` schema, the `ui` lowering, the lint rules, and the control names |

It uses `a2m-pack-web-browser` for every page. What it reads it returns as the page shows it; the engine normalizes it by the field type.

## State

| State | Where |
| --- | --- |
| Prepared tabs of runs in flight | Memory, released by `release` |
| A browser's session | The browser's own data directory, in the base's profile root; never here |

## Failures

As built. Every failure says whether input reached the page.

| Class | Code | When | Sent |
| --- | --- | --- | --- |
| `drift` | `locator_not_found` | No locator of a field, an action, or a table matches | Not sent |
| `drift` | `locator_ambiguous` | A locator matches several elements | Not sent |
| `drift` | `fingerprint_mismatch` | The element is not inside its section, the section does not show its text, or a table's headings or rows are not the declared ones | Not sent, or answered for a table |
| `unknown_variant` | `unexpected_page` | A screen that is neither ready nor in a declared variant in the step's time; an element that is hidden, disabled, or covered; an answer of the browser no rule covers | Not sent, or answered |
| `unknown_variant` | `unexpected_control` | The element cannot be operated or read as its control says; a text control that does not hold what it was given | Not sent; unknown once input reached it |
| `unknown_variant` | `unknown_control` | The plan names a control this driver has no rule for | Not sent |
| `unknown_variant` | `unexpected_dialog` | The page opened a dialog; none can be declared yet | As the base reports |
| `unknown_variant` | `read_too_large` | A read longer than `max_read_chars` | Answered |
| `unknown_variant` | `unexpected_status` | A `page_http` read answered with a status other than 200 | Answered |
| `drift` | `response_schema_mismatch` | A `page_http` answer that is not JSON, not an object, lacks a required name, holds a declared name of another kind, or an undeclared one where the entry says `additionalProperties: false` | Answered |
| `unknown_variant` | `browser_not_owned` | The base's: what started is not a browser the engine can own | Not sent |
| `rejected` | `app_validation_error` | The page shows a variant with handling `stop`: at `wait`, after a click that did not show it before, or before a click, which is then not made | Answered; not sent before a click |
| `possible_commit` | `no_confirmation` | After the click of a committing action: the confirmation did not show in time, the browser was lost, the page tried to leave, opened a window, or began a download, the page showed an `absent` variant, or both a variant and the confirmation. The handle makes every other doubtful failure of that step the same | Unknown |
| `policy` | `surface_not_declared` | The surface's address is not one the run may reach, the page ended on another origin, or the page tried to move to one, which the browser stopped | Not sent, or answered |
| `policy` | `password_read_refused` | The base's: a password input was asked for its value | Not sent |
| `policy` | `read_only_posture` | The contract's: a `set` or an `invoke` in a run that may only look, or an `invoke` that is not the commit point in a run against a test record | Not sent |
| `busy` | `browser_unavailable` | The base's: no browser is installed | Not sent |
| `input` | `step_not_usable` | A step or a payload this driver cannot carry out as written; a session whose provider hands over a credential to place | Not sent |
| `input` | `address_invalid` | The surface's `origin` is more than a scheme, a host, and a port | Not sent |
| `input` | `address_value_refused` | A value that cannot stand in one segment of a screen's address | Not sent |
| `transient` | `timeout`, `connection_lost` | The contract's: the browser did not answer, or was lost, on a step that commits nothing | As the base reports |

Not built: `edge_blocked` and its codes, `session_expired` through the lease, `stale`, `version_conflict`.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| A registry with only this pack builds, with a stable digest; the manifest declares every code the pack produces, every step kind its lowering makes, and every known place of a browser | `tests/compile.rs` |
| The fixture target compiles twice to the same bytes; locators are in the pack's section and in no step; the plan holds no address | `tests/compile.rs` |
| An unknown control, an unknown key, a locator that matches in two ways, a selector that is not last, a missing fingerprint, an address that could leave the surface, and a binding that names what its screen lacks are refused with a file, a line, and a fix | `tests/compile.rs` |
| A read gives what the form shows, and five runs do the same thing with the same result | `tests/driver.rs`, against the control interface |
| A table is read by its declared headings; an empty table is an empty list; a record that is not there is absent; a value cannot leave its segment of the address | `tests/driver.rs` |
| A changed screen is `drift`: a missing locator, an ambiguous one, a failed fingerprint. A control with no rule and a page with no rule are `unknown_variant` | `tests/driver.rs` |
| A committing step without the ledger's token saves nothing. With it, one write is stored exactly once, with the values sent, and a value the application reformats compares equal under its field type and another does not | `tests/driver.rs`, with a real store and the control interface |
| A refused write is `rejected`, answered, and stores nothing. A run that may only look touches no control. A surface the run may not reach is not opened | `tests/driver.rs` |
| A browser lost after the committing click is `possible_commit`, never `transient`, and nothing is sent again | `tests/driver.rs`: the application holds the save, the browser is killed, the control interface counts |
| A password on the page is never read, and is in no capture and no log; a snapshot masks it | `tests/driver.rs`, with a page the test serves |
| A page that moves itself to another origin stops the run before a field is set; nothing reaches the other origin | `tests/driver.rs`, `a_page_that_moves_to_another_origin_is_stopped_before_anything_is_typed`, with the test application's `leaving` pages and its traps |
| A page that leaves, or opens a window, after a committing click is `possible_commit`, while the application did store the save | `tests/driver.rs`, `a_page_that_leaves_or_opens_a_window_after_the_committing_click_is_a_write_in_doubt` |
| A variant, or the confirmation, the page showed before the click is never taken for its answer: the click is not made, and nothing is stored | `tests/driver.rs`, `a_state_the_page_showed_before_the_click_is_never_taken_for_its_answer` |
| A binding is held to the least effect of what it does | `tests/compile.rs` |
| No driver calls an auth provider or renews | The driver holds none; a lease is used for its profile only |
| A drafted read of a snapshot's table compiles, reads what the application holds, writes nothing, and drafts the same bytes twice; a drafted screen the page no longer matches stops; a table two of the page share is a question, a form drafts nothing, and a drafted screen with an action is refused | `tests/draft.rs`, against the snapshot the probe's test holds equal to discovery |
| A run that may only look has a tab that cannot write: a page that writes when it loads stops the read, and the write never reaches the application | `tests/draft.rs`, `a_run_that_may_only_look_has_a_tab_that_cannot_write` |
| An undeclared dialog stops the run | The mapping is built; no test, because the test application opens no dialog |
| A `page_http` read reads each order by key as the control interface holds it; a key that leaves its segment sends nothing; a changed answer stops by name; the page's cookie and token are nowhere in what the pack gives | `tests/page_http.rs`, with the probe's finding kept in `tests/fixtures/findings/order_totals_exchange.json` |
| `ui` steps stay under 50 ms of overhead on a warm tab | Not measured as a budget. A first measurement ([decision 0190](../../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md)), headless on Linux, the totals of one order with no delay: a whole `ui` read 118 ms, a whole `page_http` read 99 ms, the `request` step alone 9 ms, the two results equal |

## Build order

1. Done: `UiDriver` with `goto`, `wait`, and `read` for a text field, through the handle.
2. Done: the manifest, `pack` and `add_to`, the `web` surface kind, the `screens/` schema and its compilation, the `ui` lowerings for `read` and `list`, three lint rules, and the control names.
3. Done in part: reads of text, value, checked state, and table rows; variants `absent` and `stop`. Not built: frames, `open` and `search`, a row by its key, attribute reads, the conditions `value`, `url`, `response`, and `profile_ready`.
4. Done: `set` for text, select, and check box; `invoke` with the commit point, the confirmation inside it, and `possible_commit`; snapshots at the two checkpoints of a write; read-back from the page afresh.
5. Not built, package 2.3 and 2.6: the auth providers. The base has no visible window and no export of a session's state yet, and the test application has no sign-in page, so neither the sign-in window nor `form_login` can be proven. Signs of expiry are not detected. A lease is used for the name of its profile; a provider that hands over a credential to place is refused.
6. Not built: the write half of `PageHttpDriver` (package 4.2): when it is built, its lowering takes the HTTP pack's rules for the `endpoints/` kind it shares ([decision 0154](../../decisions/0154-the-http-command-and-mcp-packs-state-what-saves-what-writes-and-which-key-is-sent.md)): its request step is `AsDeclared` and every other kind `Never`, a version precondition needs a read-back endpoint that reads a version ([decision 0139](../../decisions/0139-a-write-that-was-sent-is-never-settled-by-an-answer-that-is-not-the-applications-verdict.md)), and its driver refuses an endpoint that writes from a step not marked `commits`. Also not built: `SiteToolsDriver`, platform profiles, the remaining controls, dialogs, uploads and downloads, `delete`, identity by a stamp or a search, setting a finding against existing screen models, and drafting fields that are set and `site_tools` reads.
7. Done: the drafting rules from a snapshot's tables, and the observing tab for a run that may only look ([decision 0183](../../decisions/0183-the-web-pack-drafts-a-read-of-each-table-a-snapshot-names.md)).
8. Done: reads by key over a matched screen, a placeholder's value refused unless it is one plain segment, and the `get` read of labelled values ([decision 0187](../../decisions/0187-the-web-pack-matches-a-screen-by-its-address-and-reads-by-key.md)).
9. Done: the read half of `page_http`: the mode, its lowering and driver, the `page_requests/` file kind, and the drafting rules from a page's own `exchange` ([decision 0190](../../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md)). Not built: its write half, a `token` step, `responses` other than 200, a lookup that finds a record absent, a page kept open between reads, and one catalog with the HTTP pack.

## Open points

- **`screens/` is registered twice,** by this pack for `web` surfaces and by the desktop pack for `desktop` surfaces. Whether the shared parts should become the interface archetype's, in the kernel, is for the desktop pack to show.
- **A fingerprint cannot name a role,** because the base cannot say what role an element has or whether two queries found one element ([decision 0113](../../decisions/0113-a-screens-fingerprint-is-a-section-absent-is-a-variant-and-a-table-is-read-by-its-headings.md)).
- **A failing step keeps no evidence.** A step failure carries no capture, so the snapshot of the page at a failure cannot be handed to the trace. What a step did travels as a capture named `acting`, because an outcome has no field for it.
- **The lease does not say whose session it is.** The base keys a browser by target, environment, and identity; the lease gives a session and the name of a profile. The driver makes an account name from the profile's name, and refuses a profile named as one no person owns (`anonymous`, the profile of every run with no session, and `discovery`, the probe's). Every run with no session of one target and environment shares one profile, so cookies and storage a page sets in one person's run are there in the next person's; such a surface asks for no sign-in, and its pages are held to its origin, but the profile is not one person's. No test runs with a lease.
- **The read-back is a new visit to the page, in the same profile.** For a page the server renders this is a fresh read. An application that keeps an edit in the page's own storage, and sends it later, would show its own copy, and the write would look verified before the server has it; the engine would need to read back with its cache and the page's storage set aside, which the browser engine does not offer yet.
- **Two checks are not one look at the page.** A fingerprint is checked by a second locate, and a click is made after the variants and the confirmation were asked about; the page can change in between. A locator that has its own `within` is refused with a fingerprint by the compiler, and again by the driver. A refusal the page shows a moment after the click, for a reason of its own such as a field that lost the focus, is taken as the answer; on resume, a `rejected` record is reconciled before anything is written again.
- **Page text in captures is not marked untrusted here.** A snapshot holds the page's titles and names as evidence; marking free text from a system as untrusted where it reaches a caller is the engine's.
- **A browser program named in a person's settings cannot be started,** because a scoped supervisor starts only what the manifest declares. The pack declares no such setting.
- **Restricted fields are not masked in a snapshot.** The base masks passwords and what the redactor knows; the driver does not yet tell it which fields a target marks restricted.
- How `page_requests/` is held to the endpoint catalog's form when `a2m-pack-http` changes it, until the two share one catalog.
- The tool registry a page offers is an early proposal (brief 24); its shape is settled when the standard is.
- Whether a validation message is always a declared variant, or the profile can name a generic one.
- Whether a table whose cells hold a tab or a line break needs another way of reading than the text the page shows.
