# `a2m-pack-web-ui`

The web pack's drivers and the knowledge they carry: the `ui`, `page_http`, and `site_tools` drivers, the widget drivers, platform profiles, screen models as a file kind, the auth providers that need a browser, and the drafting rules for observed interfaces. It is built on `a2m-pack-web-browser` and is called back by the engine and the session keeper.

## Facts

| | |
| --- | --- |
| Side | Pack: web |
| Layer | 10, Packs |
| May depend on | Layers 1 to 5, and `a2m-pack-web-browser` |
| Depends on | `a2m-core` (with `Plan` and `Step`), `a2m-expr` (field-type host functions for widget formatting), `a2m-files` (file-kind, lowering, lint, and drafting registration), `a2m-auth` (`AuthProvider`, `SessionLease`), `a2m-provider` (`Driver`, `Discoverer`, `SandboxPort`, `Manifest`, `PackContext`, registration), `a2m-telemetry`, `a2m-pack-web-browser`. Outside: `tokio`, `serde`, `serde_json` |
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

| Registration | Content |
| --- | --- |
| Modes | `ui`, archetype interface. `page_http` and `site_tools`, archetype request |
| Surface kinds | `web` |
| Step kinds | `goto`, `wait`, `locate`, `set`, `invoke`, `read`, `handle`, `upload`, `download`, `observe`, `token`, `request`, `validate`, `extract`, `call_tool`, each as a checked name |
| Drivers | `UiDriver`, `PageHttpDriver`, `SiteToolsDriver` |
| File kinds | Owns `screens/` for surfaces of kind `web` ([file formats](../../reference/file-formats.md#screensyaml)). Reads `endpoints/`, which `a2m-pack-http` owns, for the `page_http` lowering |
| Binding fields | `ui`: `screen`, `verb`, `fields`, `action`. `page_http`: `endpoint`, `path_params`, `query`, `body`. `site_tools`: `tool`, `args` |
| Verb lowerings | `ui`: each verb of the [verbs by mode](../../reference/technical-reference.md#verbs-by-mode) table into `goto`, `wait`, `locate`, `set`, `invoke`, `read`, `handle`, `upload`, `download`, `observe`. `page_http`: into `token`, `request`, `validate`, `extract`. `site_tools`: into `call_tool`. Each lowering compiles what it reads, its screens and, for `page_http`, the endpoint entries the compiler hands it as validated data, into its own opaque step payload, and marks the steps that commit and the steps that are lookups |
| Control names | Every control in the technical reference's table, as checked names. The widget drivers themselves are internal to this crate |
| Lint rules | Below |
| Drafting rules | Below |
| Discoverers | One: screen matching, which sets a stored snapshot finding against the target's screen models. It reads findings through `SandboxPort`, not the page |
| Auth providers | `form_login`, `sso_oidc`, `sso_saml`. All others are in `a2m-auth` |

### Lint rules

Codes are stable once released.

| Code | Level | Rule |
| --- | --- | --- |
| `web_css_not_last` | Error | A `css` locator is allowed only last in a list |
| `web_css_used` | Warning | Any `css` locator |
| `web_generated_id` | Error | A locator matches a rejected identifier pattern of the surface's profile |
| `web_no_fingerprint` | Error | A field or action with no fingerprint |
| `web_commit_unconfirmed` | Error | An action with `commits: true` and a confirmation that could be true before it acted |
| `web_variant_unhandled` | Error | A variant with no handling |
| `web_frame_undeclared` | Error | A screen whose locators need a frame and declare none |
| `web_profile_readonly` | Error | A write binding on a surface whose profile's tier is `read_only` or `unsupported` |
| `web_signed_request` | Error | A `page_http` binding on an endpoint marked as signed in the browser, used for a write before it is verified |

### Drafting rules

| From a finding of kind | The drafter writes | Left as an open question |
| --- | --- | --- |
| `screen`, from a snapshot | A screen model: `reach` from the observed address with identifier segments as placeholders, `ready` from the profile's rule or a visible heading | What the screen is for; its name beyond the mechanical one |
| `field`, from a snapshot | A field: `control` from the role by a fixed table (textbox to `text`, combobox to `combobox`, checkbox to `checkbox`); `locate` as role and name, then label; `fingerprint` from the nearest heading or section | Its field type. A field type is meaning, so the draft does not compile until it is settled |
| `action`, from a snapshot | Nothing | Every action, and whether it commits |
| A tool a page registers, marked read-only by the page | A `site_tools` binding for one read operation | Tools not marked read-only; whether the page's marking is believed |

## Interface

A sketch.

```rust
/// Called once by the product crate, with the pack context and the one `Browsers`,
/// whose observer the product crate gives the probe.
pub fn register(reg: &mut Registration, ctx: &PackContext, browsers: Browsers) -> Result<(), Failure>;

pub struct UiDriver { browsers: Browsers, widgets: Widgets }   // profiles come from the base
impl Driver for UiDriver {                                  // mode and capabilities are in the manifest
    async fn prepare(&self, ctx: &RunContext, lease: &SessionLease) -> Result<Prepared, Failure>;
    async fn act(&self, p: &mut Prepared, step: &Step, input: &Values) -> Result<ActOutcome, Failure>;
    async fn health(&self, surface: &Surface) -> Health;
    async fn release(&self, p: Prepared);
}
// PageHttpDriver and SiteToolsDriver: the same trait, over the same Browsers.

/// Inside the crate. A control's driver, over the base's element handle.
pub(crate) trait WidgetDriver: Send + Sync {
    fn control(&self) -> &'static str;                        // a name in the technical reference
    async fn set(&self, tab: &ActTab, el: &Element, v: &Value, ty: &FieldType) -> Result<Dispatch, WidgetError>;
    async fn read(&self, tab: &ActTab, el: &Element, ty: &FieldType) -> Result<Value, WidgetError>;
}

pub struct SsoInProfile;  impl AuthProvider for SsoInProfile { /* sign_in, is_signed_in, detect_expiry, refresh, sign_out, inject */ }
pub struct FormLogin;     impl AuthProvider for FormLogin { /* the same */ }
```

What the kernel gives this crate:

- `Driver` has four calls. Confirm and read-back are lists of steps that `act` carries out ([steps, snapshots, and diffs](../../reference/steps-snapshots-and-diffs.md)). A step marked as a lookup may report a record as absent. Every failure says whether the request was not sent, was answered, or is unknown: a crash after an `invoke` reached the page is unknown. The engine reaches the driver through a handle that refuses a committing step without the ledger's intent token.
- An outcome can carry outputs known before the commit point, such as a value the form shows before saving; the engine records them before the committing step runs.
- `Plan` and `Step` are data in `a2m-core`. A step's payload is opaque bytes that this crate's lowering wrote and its driver reads; the kernel stores and hashes it and never decodes it.
- `page_http` reads `endpoints/` as data. If `a2m-pack-http` is not enabled, a `page_http` binding does not compile. The endpoint entry is compiled here, into this crate's payload; no request-template type is shared.
- `SessionLease` lets a driver report an expiry signal and reach the session's live handle, here a `ProfileKey`, without holding a credential. `inject` places a token into a `page_http` request header without returning it.

### How each mode fills the loop

| Mode | Prepare | Act | Confirm | Read back |
| --- | --- | --- | --- | --- |
| `ui` | A new acting tab in the session's browser, with the target's fixed environment | `goto`, `locate`, `set` through the widget driver, `invoke` | The action's declared `confirm`, often an `observe`d response | The field again after the screen is reached afresh, or the save response with `prefer: response` |
| `page_http` | An acting tab on the surface, at its declared ready page | `token` if declared, then `request` with `fetch` from inside the page | Status and response schema | The endpoint the binding names |
| `site_tools` | An acting tab, and the list of tools the page registers, compared with the binding's tool and schema digest | `call_tool` with typed arguments | The tool's typed result | The read tool the binding names, or the `ui` field. Without one the write is `irreversible` |

### The entry bar

| # | Item | This crate's answer |
| --- | --- | --- |
| 1 | A driver for its verbs, no retries inside | The three drivers above. A widget driver's escalation from fill to typing to the native setter is one strategy in one step, chosen by the control, not a retry |
| 2 | A read-back rule for every write | As in the table above |
| 3 | Each identity strategy | `natural` and `composite`: `reach` built from the key. `stamped`: write the stamp field, then find the record through a list screen. `find`: a search screen, with the declared `settle`. `none`: the write is `irreversible` |
| 4 | A test stand | The test application, cases 1 to 27; `site_tools` needs a page that registers tools, which is a new case |
| 5 | A discoverer | Snapshots and capture are in `a2m-pack-web-probe`; screen matching is here |
| 6 | Failures mapped | Below |
| 7 | Effects and hazards | Starts the browser, by path and digest, through the supervisor its context scopes; reads and writes the profile root and a staging folder for files; network to the target's declared web surfaces for what the engine itself sends; holds sessions for `web` surfaces; changes a person's screen only for a sign-in window. Hazards come from operations, not from the driver |
| 8 | A record per new mode | None: these modes are from decision 0005 |

## Used by, and uses

| Caller | Uses |
| --- | --- |
| `a2m-provider` | `register`, and the drivers, auth providers, and discoverer it registers |
| `a2m-engine`, through the registry | The three drivers, only |
| `a2m-auth`, the session keeper, through the registry | The three auth providers, only |
| `a2m-files`, through the gathered set | Schemas, lowerings, lint rules, drafting rules |

It uses `a2m-pack-web-browser` for every page, and `a2m-expr` for formatting a value for a widget and normalizing what it reads.

## State

| State | Where |
| --- | --- |
| Prepared tabs of runs in flight | Memory, released by `release` |
| Session state of a browser sign-in | Sealed by the base, held by the keeper in the store; never here |

## Failures

| Class | Codes |
| --- | --- |
| `drift` | `locator_not_found`, `fingerprint_mismatch`, `locator_ambiguous` (new), `response_schema_mismatch` for `page_http`, `upstream_tool_changed` for `site_tools` |
| `unknown_variant` | `unexpected_dialog`, `unexpected_page`, `unexpected_status`, `unknown_platform` (new: a surface's profile is `auto` and no profile matched) |
| `rejected` | `app_validation_error`: a declared variant with handling `stop` that shows a validation message |
| `possible_commit` | `no_confirmation`: a commit point was sent, or its sending is unknown, and its confirmation did not arrive |
| `stale` | `version_conflict` |
| `edge_blocked` | `challenge_page`, `rate_limited_by_edge`, `forbidden_by_edge` |
| `session_expired` | `needs_login`, reported through the lease, after the keeper has the signal |
| `policy` | `surface_not_declared`: a navigation or in-page request to an origin the target did not declare |
| `transient` | `timeout`, `connection_lost` before anything was sent |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| An undeclared dialog or page stops the run | Test-application case 11 |
| A crash after a commit point was sent is `possible_commit`, never `transient` | A fault test that kills the browser after `invoke` |
| Every widget driver sets and reads back its control | Conformance group "Acting", cases 7, 9, 10 |
| No driver calls an auth provider or renews | A test that the drivers hold no `AuthProvider`; the keeper's ten-runs-one-renewal test |
| A form login with a stored credential is tried once | Package 2.6 test with a refused credential |
| A drafted screen carries no field type and no action the snapshot did not state | Drafting tests in package E4 |
| `ui` steps stay under 50 ms of overhead, `page_http` under 20 ms, on a warm tab | Benchmarks |

## Build order

1. Slice 1.1: `UiDriver` with `goto`, `locate`, `read` for a text field; a hand-written plan.
2. Slice 1.3: `register` with the `screens/` schema and the `ui` read lowerings.
3. Slice 1.6: every read verb, variants, frames, the read half of each widget driver.
4. Package 2.3: `SsoInProfile` as a person signing in in a visible window of the engine's own browser. Package 2.6: `form_login`, silent SSO, expiry rules.
5. Slices 3.3 onward: `set`, `invoke`, `observe`, the write half of the Phase 3 widgets.
6. Package 4.2: `PageHttpDriver`. Package E4: the drafting rules and screen matching.
7. Package 8.4: platform profiles and the remaining widgets. Then `SiteToolsDriver`.

## Open points

- **`screens/` is registered twice,** by this pack for `web` surfaces and by the desktop pack for `desktop` surfaces. Whether the shared parts should become the interface archetype's, in the kernel, is for the desktop pack to show.
- How the `page_http` lowering is held to the `endpoints/` schema version it was written for, when `a2m-pack-http` changes it.
- The tool registry a page offers is an early proposal (brief 24); its shape is settled when the standard is.
- Whether a validation message is always a declared variant, or the profile can name a generic one.
