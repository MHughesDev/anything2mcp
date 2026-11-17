# `a2m-pack-web-ui`

The web pack's drivers and the knowledge they carry: the `ui`, `page_http`, and `site_tools` drivers, the widget drivers, platform profiles, screen models as a file kind, the auth providers that need a browser, and the drafting rules for observed interfaces. It is built on `a2m-pack-web-browser` and is called back by the engine and the session keeper.

Built so far: the `ui` driver for reads and verified writes, the `screens/` file kind, and the `ui` lowering. Everything else on this page is the design of what follows, and the build order says which is which.

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
| Modes | `ui`, archetype interface, rank 60, four reads and one write at a time on a session. `page_http` and `site_tools` are not built |
| Surface kinds | `web`, with the address key `origin` and the fields `viewport` (`width`, `height`), `color_scheme` (`light` or `dark`), and `reduced_motion`. `profile` is not accepted yet |
| Step kinds | `goto`, `wait`, `read`, `set`, `invoke` ([decision 0112](../../decisions/0112-the-ui-driver-as-built-five-step-kinds-and-a-commit-that-waits-for-its-confirmation.md)). `handle`, `upload`, `download`, `observe`, and the kinds of the other two modes are not built |
| Drivers | `UiDriver`, over the `Browsers` of the cell the product crate hands the pack |
| File kinds | Owns `screens/` for surfaces of kind `web` ([file formats](../../reference/file-formats.md#screensyaml), [decision 0113](../../decisions/0113-a-screens-fingerprint-is-a-section-absent-is-a-variant-and-a-table-is-read-by-its-headings.md)) |
| Binding fields | `ui`: `screen`, `fields`, `action`, and `read_back` with a `screen` |
| Verb lowerings | `ui`: `read`, `list`, `create`, `update`, `upsert`, `submit`, below. `open`, `search`, `delete`, `upload`, and `download` are not built |
| Control names | `text`, `textarea`, `number`, `currency`, `measurement`, `select`, `checkbox`. The compiler refuses any other with `kernel.unknown_control` |
| Lint rules | Below |
| Settings | `packs.web.max_read_chars` and `packs.web.max_snapshot_elements` |
| Effects | Writes the root `profiles`; network to surfaces; sessions for `web` surfaces; a requirement that lists every path of the browser engine's `KNOWN_PLACES` |
| Drafting rules, discoverers, auth providers | None yet |

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
| `web.signed_request` | Error | A `page_http` binding on an endpoint marked as signed in the browser, used for a write before it is verified | No: `page_http` |

The file kind and the lowering refuse, under the same prefix: `locator_invalid`, `condition_invalid`, `within_and_section`, `read_not_fitting`, `column_unknown`, `reach_invalid`, `unknown_screen`, `unknown_field`, `unknown_action`, `placeholder_unbound`, `position_type`, `value_type`, `action_missing`, `read_acts`, `read_back_never_absent`, and `not_encodable`.

### Drafting rules

Not built. As designed:

| From a finding of kind | The drafter writes | Left as an open question |
| --- | --- | --- |
| `screen`, from a snapshot | A screen model: `reach` from the observed address with identifier segments as placeholders, `ready` from the profile's rule or a visible heading | What the screen is for; its name beyond the mechanical one |
| `field`, from a snapshot | A field: `control` from the role by a fixed table (textbox to `text`, combobox to `combobox`, checkbox to `checkbox`); `locate` as role and name, then label; `fingerprint` from the nearest heading or section | Its field type. A field type is meaning, so the draft does not compile until it is settled |
| `action`, from a snapshot | Nothing | Every action, and whether it commits |
| A tool a page registers, marked read-only by the page | A `site_tools` binding for one read operation | Tools not marked read-only; whether the page's marking is believed |

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
| `goto` | A screen | `reach.<placeholder>` for each placeholder of its address | Builds the address from the surface's origin and the screen's path, each value one percent-encoded segment, and sends the tab there. A page that ends on another origin is `policy`, `surface_not_declared` |
| `wait` | A screen | None | Waits, on the base's observer, until the screen's `ready` holds or one of its variants shows. A variant `absent` is the outcome absent, for a lookup; a variant `stop` is `rejected`. In a write binding it keeps a snapshot |
| `read` | A screen | None | For each output, finds the field of its name and reads it by its control's rule, as the text the page shows; for a list, reads the screen's table by its headings |
| `set` | A screen | `value` | Finds the field and sets it by its control's rule |
| `invoke` | A screen | None | Finds the action, clicks it, and waits until the page shows the action's `confirm` or a variant. Marked as the commit point when the action says `commits`. Keeps a snapshot |

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
- `page_http` reads `endpoints/` as data. If `a2m-pack-http` is not enabled, a `page_http` binding does not compile. The endpoint entry is compiled here, into this crate's payload; no request-template type is shared.
- `SessionLease` lets a driver report an expiry signal and reach the session's live handle, here a `ProfileKey`, without holding a credential. `inject` places a token into a `page_http` request header without returning it.

### How each mode fills the loop

| Mode | Prepare | Act | Confirm | Read back |
| --- | --- | --- | --- | --- |
| `ui` | A new acting tab in the session's browser, with the target's fixed environment | `goto`, `wait`, `set` through the control's rule, `invoke` | The action's declared `confirm`, inside the committing `invoke`; the confirm list is empty | The fields again after the screen is reached afresh. `prefer: response` is not built |
| `page_http` | An acting tab on the surface, at its declared ready page | `token` if declared, then `request` with `fetch` from inside the page | Status and response schema | The endpoint the binding names |
| `site_tools` | An acting tab, and the list of tools the page registers, compared with the binding's tool and schema digest | `call_tool` with typed arguments | The tool's typed result | The read tool the binding names, or the `ui` field. Without one the write is `irreversible` |

### The entry bar

| # | Item | This crate's answer |
| --- | --- | --- |
| 1 | A driver for its verbs, no retries inside | The three drivers above. A widget driver's escalation from fill to typing to the native setter is one strategy in one step, chosen by the control, not a retry |
| 2 | A read-back rule for every write | As in the table above |
| 3 | Each identity strategy | `natural` and `composite`: `reach` built from the key; built. `stamped`: write the stamp field, then find the record through a list screen; not built. `find`: a search screen, with the declared `settle`; not built. `none`: the write is `irreversible` |
| 4 | A test stand | The test application, cases 1 to 27; `site_tools` needs a page that registers tools, which is a new case. As built, the application has cases 1 to 4: its line-item form and its tables. A page with a password, a check box, and a select is served by the test that needs it |
| 5 | A discoverer | Snapshots and capture are in `a2m-pack-web-probe`; screen matching is here |
| 6 | Failures mapped | Below |
| 7 | Effects and hazards | Starts the browser, by path and digest, through the supervisor its context scopes; reads and writes the profile root and a staging folder for files; network to the target's declared web surfaces for what the engine itself sends; holds sessions for `web` surfaces; changes a person's screen only for a sign-in window. Hazards come from operations, not from the driver |
| 8 | A record per new mode | None: these modes are from decision 0005 |

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
| `unknown_variant` | `browser_not_owned` | The base's: what started is not a browser the engine can own | Not sent |
| `rejected` | `app_validation_error` | The page shows a variant with handling `stop` | Answered |
| `possible_commit` | `no_confirmation` | After the click of a committing action: the confirmation did not show in time, or the browser was lost. The handle makes every other doubtful failure of that step the same | Unknown |
| `policy` | `surface_not_declared` | The surface's address is not one the run may reach, or the page ended on another origin | Not sent, or answered |
| `policy` | `password_read_refused` | The base's: a password input was asked for its value | Not sent |
| `policy` | `read_only_posture` | The contract's: a `set` or an `invoke` in a run that may only look | Not sent |
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
| No driver calls an auth provider or renews | The driver holds none; a lease is used for its profile only |
| An undeclared dialog stops the run | The mapping is built; no test, because the test application opens no dialog |
| `ui` steps stay under 50 ms of overhead on a warm tab | Not measured |

## Build order

1. Done: `UiDriver` with `goto`, `wait`, and `read` for a text field, through the handle.
2. Done: the manifest, `pack` and `add_to`, the `web` surface kind, the `screens/` schema and its compilation, the `ui` lowerings for `read` and `list`, three lint rules, and the control names.
3. Done in part: reads of text, value, checked state, and table rows; variants `absent` and `stop`. Not built: frames, `open` and `search`, a row by its key, attribute reads, the conditions `value`, `url`, `response`, and `profile_ready`.
4. Done: `set` for text, select, and check box; `invoke` with the commit point, the confirmation inside it, and `possible_commit`; snapshots at the two checkpoints of a write; read-back from the page afresh.
5. Not built, package 2.3 and 2.6: the auth providers. The base has no visible window and no export of a session's state yet, and the test application has no sign-in page, so neither the sign-in window nor `form_login` can be proven. Signs of expiry are not detected. A lease is used for the name of its profile; a provider that hands over a credential to place is refused.
6. Not built: `PageHttpDriver` (package 4.2), `SiteToolsDriver`, platform profiles, the remaining controls, dialogs, uploads and downloads, `delete`, identity by a stamp or a search, the drafting rules and screen matching.

## Open points

- **`screens/` is registered twice,** by this pack for `web` surfaces and by the desktop pack for `desktop` surfaces. Whether the shared parts should become the interface archetype's, in the kernel, is for the desktop pack to show.
- **A fingerprint cannot name a role,** because the base cannot say what role an element has or whether two queries found one element ([decision 0113](../../decisions/0113-a-screens-fingerprint-is-a-section-absent-is-a-variant-and-a-table-is-read-by-its-headings.md)).
- **A failing step keeps no evidence.** A step failure carries no capture, so the snapshot of the page at a failure cannot be handed to the trace. What a step did travels as a capture named `acting`, because an outcome has no field for it.
- **The lease does not say whose session it is.** The base keys a browser by target, environment, and identity; the lease gives a session and the name of a profile. The driver makes an account name from the profile's name. No test runs with a lease.
- **A browser program named in a person's settings cannot be started,** because a scoped supervisor starts only what the manifest declares. The pack declares no such setting.
- **Restricted fields are not masked in a snapshot.** The base masks passwords and what the redactor knows; the driver does not yet tell it which fields a target marks restricted.
- How the `page_http` lowering is held to the `endpoints/` schema version it was written for, when `a2m-pack-http` changes it.
- The tool registry a page offers is an early proposal (brief 24); its shape is settled when the standard is.
- Whether a validation message is always a declared variant, or the profile can name a generic one.
- Whether a table whose cells hold a tab or a line break needs another way of reading than the text the page shows.
