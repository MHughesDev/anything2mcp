# `a2m-provider`

The one doorway between the kernel and everything that knows a kind of system. It holds the manifest and its checks, the registry that packs and plug-ins fill, the extension-point traits, the supervisor that starts and stops every outside process, and the plug-in host. It knows no kind of system itself.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 5, Contract |
| May depend on | Layers 1 to 4 |
| Depends on | `a2m-core`, including `Plan` and `Step` as data; `a2m-files` for the compiler registration traits only; `a2m-auth` for `SessionLease` and `AuthProvider`; `a2m-sources` for `SourceReader`; `a2m-ledger` for `IntentToken`; `a2m-telemetry` for spans. Outside: `serde`, a TOML parser and a JSON Schema validator, each an outside crate to be verified |
| Built in | Stage B, package B3 (manifest, registry); slice 1.1 (`Driver`); slice 1.3 (compiler registrations); Stage E, package E1 (`Discoverer`, `Describer`); package F3 and Phase 8 (plug-in host) |
| Carries out | Spec sections [55](../../spec/55-kernel-and-packs.md), [56](../../spec/56-the-provider-contract.md), [57](../../spec/57-supervising-subsystems.md), [58](../../spec/58-discovery-and-findings.md) for the discoverer's side, [61](../../spec/61-effects-hazards-and-consent.md) for effects; the [driver interface](../../reference/technical-reference.md#driver-interface) and the [plug-in protocol](../../reference/technical-reference.md#plug-in-protocol) |

## Owns

- The manifest: its file format, its schema, and the checks that a provider asks for nothing it did not declare.
- The `Registry`: one immutable snapshot of every provider, its modes with their archetypes, and the objects each supplied.
- The traits `Driver`, `Discoverer`, `Describer`, and `ProviderAction`, and the `DriverHandle` through which alone the engine reaches a driver.
- The registered names a provider brings: modes with their archetypes, surface kinds, step kinds, file kinds, lint codes, finding kinds. Each is a checked name from `a2m-core`; the registry is the only list of them.
- The choice between two providers that could fill the same place, and its recorded reason.
- File access for packs: `CheckedPath`, which only the check against the allowed roots can make ([section 51](../../spec/51-defending-against-a-manipulated-agent.md), [section 61](../../spec/61-effects-hazards-and-consent.md)).
- The supervisor: the only way anything starts a process, with the handshake, the framing, deadlines, and ownership-proven stopping.
- The plug-in host, and the plug-in protocol's message list and version.
- The availability check of section 56, which inspects the machine and starts nothing.

It must never: decide whether something may happen; sign in or renew; write a ledger row; open the vault; retry a step; hold a list of modes or name a pack; let a caller's input choose a program, a file, or a destination.

## The manifest

One file per provider, `anything2mcp-provider.toml`. A first-party pack embeds the same file with `include_str!`, so packs and plug-ins pass one parser and one schema. TOML was chosen over YAML because a manifest is configuration a person reviews before enabling, read by machines on every start, and TOML has no anchors, aliases, or implicit typing; target files stay YAML, settings stay TOML ([decision 0014](../../decisions/0014-declarative-files-yaml-targets-toml-settings-cel-expressions.md)). The file is checked against a JSON Schema kept in this crate.

```toml
manifest_format = 1                      # this file's own format number
contract = 1                             # the provider contract it was written for; current and previous are accepted

[provider]
name = "example_sheet"                   # checked name; also the name space of its actions
version = "0.3.1"
publisher = "Example Labs"
kind = "plugin"                          # "pack" | "plugin"
digest = "sha256:9f2c..."                # of the program that runs; for a pack, filled in at build time
signature = "anything2mcp-provider.toml.sig"   # plug-ins only; detached, over this file and the program

[run]                                    # plug-ins only: how the host starts it
program = "bin/example-sheet"            # relative to the manifest; never searched on PATH
args = ["--serve"]                       # fixed; nothing from a caller is ever added
environment = ["LANG"]                   # an allowlist of names passed through
protocol = [2, 1]                        # plug-in protocol versions it speaks, newest first
max_frame_bytes = 1048576
concurrency = 1                          # requests it serves at once; 1 means one at a time
queue = 16                               # requests waiting beyond that, before `busy`

[[modes]]
name = "sheet"                           # a Mode; registering it needs a decision record for a pack, admin approval for a plug-in
archetype = "artifact"                   # one of the seven
verbs = ["read", "list", "update"]
steps = ["open", "locate", "set", "read", "save"]
rank = 40                                # default place in binding order when an operation states none; lower is tried first
intent_window = 1                        # intents stored ahead of acting, by default
session_reads = 4                        # parallel reads per session, by default
session_writes = 1                       # parallel writes per session, by default
exclusive = "file"                       # what one write holds alone: "none", "surface", "file", "screen", "link"

[[surface_kinds]]                        # registered names, like modes; a target's surface must name one that is registered
name = "file"
archetype = "artifact"
session = false                          # whether a session of the keeper is held on it

[points]                                 # the extension points it fills; nothing else can be registered
driver = ["sheet"]                       # modes it drives
discoverer = ["sheet_structure"]         # each discoverer also supplies its describer
auth_provider = []
source_reader = ["example_sheet_rows"]
widget_driver = []

[[discoverers]]
name = "sheet_structure"
archetype = "artifact"
kinds = ["schema", "field"]              # finding kinds it may return
sources = ["live_system", "artifact"]
max_level = "discovered"
bounds = { seconds = 60, bytes = 50000000, depth = 8 }

[[file_kinds]]
name = "sheets"
directory = "sheets/"
schema = "schemas/sheets.json"           # shipped beside the manifest
[[binding_fields]]
mode = "sheet"
schema = "schemas/binding.json"

[[lint_rules]]
code = "SHEET001"
level = "error"
summary = "A named place must exist in the template fingerprint"

[[drafting_rules]]
finding_kind = "schema"
file_kind = "sheets"

[[actions]]
name = "example_sheet.inspect"           # under its own name space; faces are generated
effect = "read"

[effects]                                # each shown as enforced, confined, or declared
processes = []                           # programs it starts itself: [{ path = "...", digest = "sha256:..." }]
plan_programs = false                    # true: it may also start programs a published plan names, with their digests
read_roots = ["project", "sources"]      # named roots the kernel resolves; never a free path
write_roots = ["artifact_out"]
network = "surfaces"                     # "none" | "surfaces": only the target's declared surfaces
sessions = []                            # kinds of surface it holds a session for
target_code = false
attach = false
screen = false
devices = false

[[requirements]]
name = "example-runtime"
detect = { paths = ["/usr/lib/example-runtime/bin/ert", "C:/Program Files/Example Runtime/ert.exe"] }
known_digests = ["sha256:41ab..."]
versions = ">=2.1, <3"
missing = "Install Example Runtime 2.x from its publisher."

[platforms]
supported = ["linux-x86_64", "linux-aarch64", "macos-aarch64", "windows-x86_64"]

[evidence]
max_level = "discovered"
vouches_for = ["live_system", "artifact"]
```

Checks made when a manifest is read, each a named failure:

| Check | Failure |
| --- | --- |
| The file parses and fits the schema | `input`, `manifest_invalid`, with the field and line |
| `contract` is current or previous | `input`, `contract_unsupported` |
| Every mode and surface kind has exactly one archetype; two providers that register one name agree on it | `input`, `mode_conflict` |
| Every step kind, file kind, lint code, and finding kind is a well formed checked name, unique to its provider | `input`, `schema_invalid` |
| Every object registered matches a declared point, mode, or discoverer | `policy`, `effect_not_declared` |
| A plug-in's signature verifies, or unsigned plug-ins are switched on by a person | `policy`, `signature_invalid` |
| A discoverer's `max_level` is `inferred` or `discovered` | `input`, `manifest_invalid` |
| A program in `run` or `effects.processes` is a relative path inside the provider or a fixed absolute path | `input`, `path_not_allowed` |

## Interface

```rust
// The registry. Built once at start by the product crate; replaced whole when a provider is enabled or disabled.
pub struct RegistryBuilder { /* settings, collected registrations, problems */ }
impl RegistryBuilder {
    pub fn new(settings: ProviderSettings) -> Self;           // which providers are enabled; which provider fills a contested mode
    /// One call per enabled first-party pack. The product crate makes no other call per pack.
    pub fn add_pack(&mut self, pack: &dyn Pack);
    /// A plug-in found on the machine: built in, through an entry point, or in the plug-in directory.
    pub fn add_plugin(&mut self, found: FoundManifest);
    /// Every problem at once. A provider with a problem is listed with it and registers nothing.
    pub fn build(self) -> (Registry, Vec<RegistryProblem>);
}

pub trait Pack: Send + Sync {
    fn manifest(&self) -> &'static str;                        // the TOML text
    fn register(&self, to: &mut Registrar<'_>) -> Result<(), Failure>;
}

/// What a pack hands over. Each call is checked against the manifest as it is made.
pub struct Registrar<'a> { /* the provider being registered */ }
impl Registrar<'_> {
    pub fn driver(&mut self, mode: Mode, driver: Arc<dyn Driver>);
    pub fn discoverer(&mut self, d: Arc<dyn Discoverer>, describer: Arc<dyn Describer>);
    pub fn auth_provider(&mut self, p: Arc<dyn AuthProvider>);         // trait from `a2m-auth`; only those needing a surface
    pub fn source_reader(&mut self, r: Arc<dyn SourceReader>);         // trait from `a2m-sources`
    pub fn compiler(&mut self, c: CompilerRegistrations);              // from `a2m-files`: file kinds, lowerings, lint rules, drafting rules
    pub fn action(&mut self, a: Arc<dyn ProviderAction>);              // under the provider's name space
}                                                                      // surface kinds and step kinds come from the manifest alone

/// An action a provider adds. Its four faces are generated by `a2m-actions`, whose dispatcher alone calls it,
/// after the caller's credential is checked and policy has decided. It gets checked paths and the registry, never the store.
#[async_trait]
pub trait ProviderAction: Send + Sync {
    fn name(&self) -> &ActionName;                                     // as declared in the manifest
    fn input_schema(&self) -> &JsonSchema;
    fn output_schema(&self) -> &JsonSchema;
    async fn handle(&self, input: &Values, ctx: &ActionContext<'_>) -> Result<Values, Failure>;
}

pub struct Registry { /* immutable; cheap to clone behind an Arc */ }
impl Registry {
    pub fn digest(&self) -> Digest;                            // keys the tool-list cache and is recorded in every trace
    pub fn archetype(&self, mode: &Mode) -> Option<Archetype>;
    pub fn surface_kind(&self, kind: &SurfaceKind) -> Option<&SurfaceKindEntry>;   // archetype, whether it holds a session, which provider
    pub fn action(&self, name: &ActionName) -> Option<Arc<dyn ProviderAction>>;    // for the dispatcher in `a2m-actions`
    pub fn mode_defaults(&self, mode: &Mode) -> Option<&ModeDefaults>;   // rank, intent window, session limits, exclusive
    /// The driver for a mode, with the provider choice already made and its reason attached.
    pub fn driver(&self, mode: &Mode) -> Result<DriverHandle, Failure>;
    pub fn source_reader(&self, format: &SourceFormat) -> Result<Arc<dyn SourceReader>, Failure>;
    pub fn discoverers_for(&self, surface: &SurfaceDescription) -> Vec<DiscovererHandle>;
    pub fn auth_providers(&self) -> Vec<Arc<dyn AuthProvider>>;          // handed to the keeper by `a2m-actions`
    pub fn compiler_set(&self) -> &CompilerSet;                         // handed to the compiler by `a2m-actions`
    pub fn listing(&self) -> Vec<ProviderListing>;                      // manifest, availability, health; for `provider.list`
    pub fn supervisor(&self) -> &Supervisor;
}
```

**There is no priority and no silent fallback.** If one mode is driven by two ready providers and no setting names one, `driver` fails with `input`, `provider_ambiguous`, and the message lists each candidate with its availability. If the named one is not ready, it fails with `busy`, `provider_unavailable`, carrying the availability state and its fix; it never falls to the other. Registration order never affects the result, and a test builds the registry in every order and compares digests.

**Declared against observed.** `ProviderListing` carries three separate answers: the manifest (what it can do), the availability (can it here, now), and the health (is it working). Availability is computed by looking for each requirement at its declared paths and taking its digest. It starts no process. Its states are the eight of section 56; the subsystem states of section 57 are separate. Both are status words, so they belong in `a2m-core` as enums with a test against the documents.

### The extension points

The driver interface is settled here as follows, closing the two points the technical reference leaves to slice 1.1.

1. **Confirm and read-back are lists of steps, carried out by `act`.** There is no `read_back` method. The compiler lowers a binding into three step lists, act, confirm, and read-back, and the engine passes each step to `act`. The plan then says everything a driver will do, the trace has one record per step whatever the part of the loop, and the preview, the freshness check, and reconcile run the read-back list on its own exactly as file formats require. A separate method would let a pack hide reading logic the plan does not show, and would give two ways to read a record.
2. **A step can report a record as absent.** `StepOutcome::Absent` is allowed only for a step the plan marks `lookup`. Anywhere else it is refused by the handle as `unknown_variant`, `unexpected_absent`.
3. **Declared capabilities leave the trait.** The sketch's `mode()` and `capabilities()` repeat the manifest; knowledge lives in one place, so the registry answers both.
4. **Every failure says whether anything was sent,** as the supervisor's rule asks of every subsystem.
5. **A step can return what reconcile will need before the commit point.** A key a store assigned inside an open transaction, or the digest a file will have once it is replaced, is learned before the step that commits. A step not marked `commits` may return it as `Observed::for_reconcile`; the engine records it durably through `Control` before the committing step is sent. The handle refuses `for_reconcile` on the committing step itself, where it would be too late to be safe.

```rust
#[async_trait]
pub trait Driver: Send + Sync {
    /// Acquire what the binding needs: a tab, a client, a window, an open file. `lease` is None for a surface with no session.
    async fn prepare(&self, ctx: &PrepareContext, lease: Option<&SessionLease>) -> Result<Prepared, StepFailure>;
    /// Carry out exactly one step of the kinds this mode declared. Never retries. Values are already evaluated.
    async fn act(&self, p: &mut Prepared, step: &Step, input: &StepInput) -> Result<StepOutcome, StepFailure>;
    /// From memory, without input or output: may this driver be chosen for this surface now.
    fn health(&self, surface: &SurfaceRef) -> DriverHealth;
    /// Give back what prepare acquired. `poisoned` after a failure: recycle, never reuse.
    async fn release(&self, p: Prepared, poisoned: bool) -> Cleanup;
}

pub struct Prepared(Box<dyn Any + Send>);                      // the driver's own state; opaque to the kernel
pub struct PrepareContext<'a> {
    pub run: &'a RunId, pub surface: &'a SurfaceRef, pub posture: Posture,
    pub deadline: Deadline, pub redact: &'a dyn Redact,
    pub destinations: &'a Destinations,                       // the target's declared surfaces, within the manifest's network effect
    pub paths: &'a CheckedPaths,                              // every file this run may touch, already checked; a driver resolves none
    pub supervisor: &'a ScopedSupervisor,                     // starts only what the manifest and the published plan declare
    pub widgets: &'a WidgetSet,                               // empty until plug-in widget drivers exist
}
pub enum Posture { Live, Sandbox { test_records: bool }, Discovery }   // the driver applies its own kind's read-only guard outside Live
pub enum StepOutcome { Done(Observed), Absent(Observed) }
pub struct Observed {
    pub values: Values, pub assigned_key: Option<RecordKey>,
    pub for_reconcile: Values,                                // learned before the commit point; recorded before the committing step
    pub captures: Vec<Capture>,                               // already redacted
}

/// File access. A `CheckedPath` is made only by `Roots::check`: a named root, a relative path, no `..`, no link leaving
/// the root, resolved once. `a2m-actions` uses the same check for the kernel's own reads.
pub struct Roots { /* named roots from settings and the project, each with read or write */ }
impl Roots { pub fn check(&self, root: &RootName, relative: &str, access: Access) -> Result<CheckedPath, Failure>; }
pub struct CheckedPath(/* private */);                        // no public constructor; opened only by its own methods
pub struct StepFailure { pub failure: Failure, pub sent: Sent }
pub enum Sent { NotSent, Unknown, Answered }                   // Answered: the system replied, so the outcome is known
pub enum DriverHealth { Ready, Degraded(FailureCode), Unavailable(FailureCode) }

/// The only way the engine reaches a driver. Not constructible outside this crate.
pub struct DriverHandle { /* Arc<dyn Driver>, archetype, provider choice */ }
impl DriverHandle {
    pub fn choice(&self) -> &ProviderChoice;                   // provider, version, digest, reason: recorded in the trace
    pub async fn prepare(&self, ctx: &PrepareContext<'_>, lease: Option<&SessionLease>) -> Result<Prepared, StepFailure>;
    /// A step marked `commits` needs `Authority::Write` with a token from `a2m-ledger`, or it is refused unsent.
    pub async fn act(&self, p: &mut Prepared, step: &Step, input: &StepInput, auth: Authority<'_>) -> Result<StepOutcome, StepFailure>;
    pub fn health(&self, surface: &SurfaceRef) -> DriverHealth;
    pub async fn release(&self, p: Prepared, poisoned: bool) -> Cleanup;
}
pub enum Authority<'a> { Read, Write(&'a IntentToken) }

#[async_trait]
pub trait Discoverer: Send + Sync {
    /// From the manifest and the surface description alone: does it apply here.
    fn suits(&self, surface: &SurfaceDescription) -> bool;
    /// Observe within `bounds`, reaching the system only through `port`. Findings go to `out` as they are made.
    async fn discover(&self, port: &dyn SandboxPort, scope: &DiscoveryScope, out: &mut dyn FindingSink)
        -> Result<DiscoveryReport, Failure>;           // report: bounds hit, coverage, counts dropped
}

/// Implemented by the engine's sandbox; a write cannot pass through it.
#[async_trait]
pub trait SandboxPort: Send + Sync {
    async fn act(&self, step: &Step, input: &StepInput) -> Result<StepOutcome, StepFailure>;
}

pub trait Describer: Send + Sync {
    /// Pure: findings in, text and structure an agent can read out. No input or output; untrusted text stays marked.
    fn describe(&self, findings: &[Finding], form: DescribeForm) -> Description;
}
```

Contracts the signatures do not show:

- **The handle is the enforcement point.** Before calling the pack it checks: the step's kind is one the mode declared; a `commits` step carries a token whose run, record key, and lease number match the context; the posture is `Live`, or `Sandbox` with a token for a declared test record. A refused step is reported `Sent::NotSent`.
- **The handle fixes the class of a doubtful write.** A failure on a `commits` step with `Sent::Unknown` becomes `possible_commit`, whatever class the driver gave. A driver cannot make a doubtful write look retryable.
- **A failure's class is one of the twelve, and its code is declared.** A code the provider's manifest and the reference do not name is refused as `unknown_variant`, `undeclared_code`.
- **Cancellation.** A driver must be safe to cancel on any step not marked `commits`. On a `commits` step the engine waits to the deadline and never drops the future.
- **No secret leaves a driver.** Credentials arrive through the lease's injection, assumed to be a method of `SessionLease` that writes into a sink the driver owns and never returns the value. Captures pass the `Redact` the context carries before they leave `act`.
- **A discoverer has no handle and no token.** A runner calls it inside the engine's sandbox for a discovery job, and it reaches the system only through `SandboxPort`. For a first-party pack that is structural and reviewed; for a plug-in it is only as strong as the effects listing says. What a read-only step is, for its archetype, each pack states and its conformance test proves.

## The supervisor

```rust
pub struct Supervisor { /* places, counts, owned groups, cleanup states */ }
impl Supervisor {
    pub async fn start(&self, spec: LaunchSpec) -> Result<Subsystem, Failure>;
    pub fn list(&self) -> Vec<SubsystemListing>;               // identifier, kind, since when, for whom, state
    pub async fn stop(&self, id: SubsystemId) -> Cleanup;
    pub async fn shutdown(&self) -> Cleanup;                   // every caller that asks joins the same cleanup
}
pub struct LaunchSpec { pub component: ComponentRef, pub args: Vec<OsString>, pub env: EnvAllowlist,
                        pub cwd: RootRef, pub link: LinkKind, pub place: PlaceKey, pub kind: SubsystemKind }
pub enum LinkKind {
    Framed { max_frame_bytes: u32 },       // our framing and the full handshake: plug-ins, bridges
    Foreign,                               // the program's own protocol, framed by its pack: a browser's debugging pipe
    OneShot { max_output_bytes: u64 },     // run to completion, no handshake: a command; exit status and bounded output
}
/// What a pack's driver gets. Its allowlist is the manifest's `processes`, plus, where `plan_programs` is true,
/// the programs the published plan names with their digests. Anything else is `effect_not_declared`.
pub struct ScopedSupervisor { /* the supervisor, the provider, the plan's programs */ }
pub struct Subsystem { pub id: SubsystemId, pub conn: Connection }
impl Connection {
    /// One request, one reply, under a deadline. The failure says whether the request ran.
    pub async fn request(&self, msg: Frame, deadline: Deadline) -> Result<Frame, ConnFailure>;
}
pub struct ConnFailure { pub failure: Failure, pub ran: Ran }
pub enum Ran { NotStarted, Unknown }
pub struct Cleanup { pub complete: bool, pub remaining: Vec<Leftover> }
```

| Rule | How it is held |
| --- | --- |
| Identify before starting | The program is found at a declared place and digested. An unknown digest is recorded as unverified, not refused, and the mark travels with everything the subsystem produces |
| A private handshake | Two random values per launch: an owner mark, 128 bits, placed in the child's environment for stopping; and a token, 256 bits, written as the first line on the child's standard input and nowhere else. Never on a command line. The child's first frame must return the launch identifier, a keyed hash of it under the token, its version, its manifest digest, the limits it honors, and what it is attached to. Anything else closes the connection as `unknown_variant`, `handshake_failed` |
| A group the runtime can end | A process group on POSIX; a job object set to end with its owner on Windows |
| One framing, one reader | Newline-delimited JSON in UTF-8, each line at most the ceiling agreed at the handshake: the lower of both offers, 1 MiB by default, 16 MiB at most. One reader per connection matches replies to requests by identifier |
| A broken stream is not repaired | An unknown reply, an oversized line, malformed text, or a reply to no request makes the connection untrusted. It is closed, the subsystem stopped, and every pending request fails with `Ran::Unknown` |
| Every request has a deadline | From the step's time limit. Nothing waits without one |
| One at a time where it must be | Requests queue in order up to the manifest's `queue`; a full queue answers `busy`, `queue_full` at once. A cancelled request leaves the subsystem shown as busy until it answers or is restarted |
| Counted, and one start per place | Each kind has a limit. A stopped subsystem holds its place until released. Two callers asking for one place get one start |
| Ownership proven at stopping | On POSIX each process in the group is checked for the owner mark and its start time; on Windows, membership of the job object the supervisor holds is the proof. A process that cannot be checked is neither skipped nor killed: cleanup is incomplete |
| Incomplete cleanup is a state | It names what remains, blocks new starts of that kind with `busy`, `cleanup_incomplete`, and is shown by `doctor.run` |
| Fixed system paths | The process list is read through the operating system's own interfaces, never through a program found on a search path |

Two cases section 57 does not cover:

| Link | Identified and owned by | What the supervisor still does |
| --- | --- | --- |
| `Foreign`: a protocol someone else fixed | The digest, the version the program reports through its own protocol, and the owner mark. It cannot return the launch identifier | Starts, counts, bounds, and stops it; the pack frames the bytes, and a frame the pack cannot parse still closes the link |
| `OneShot`: a program run to completion, such as a command | The digest and the owner mark; there is no handshake | Arguments as a list, never a shell line; the environment allowlist; a checked working directory; output cut at its bound and counted; the deadline; the whole group ended at exit or deadline. A deadline passed on a committing run is `Ran::Unknown` |

## The plug-in host and protocol

The host reads a plug-in's manifest at build time and registers proxies: `PluginDriver`, `PluginDiscoverer`, `PluginDescriber`, `PluginAuthProvider`, `PluginSourceReader`, and compiler registrations whose lowerings and lint rules are calls. The process is started under the supervisor on first use, never at registration. Schemas cross the boundary as the files the manifest names; lowerings and lint rules cross as calls made at compile time, and the plan records the plug-in's digest, so a changed plug-in is a changed plan.

Protocol version 1. JSON-RPC 2.0 objects inside the shared framing. An error carries `data: {class, code, message, next_step, sent}`.

| Message | From | Purpose |
| --- | --- | --- |
| launch line | Host | First line on standard input: launch identifier and token |
| `hello` | Host, request | Protocol versions offered, contract version, frame ceiling. The reply is the handshake |
| `shutdown` | Host, request | Finish nothing new and exit; the host then stops the group |
| `cancel` | Host, notification | Stop work on a request identifier if it can; the subsystem stays busy until it replies |
| `receipt` | Host, request | Did request N complete: `completed`, `not_seen`, or `in_progress` |
| `compile.lower` | Host, request | One binding's verb in, its act, confirm, and read-back step lists out |
| `compile.lint` | Host, request | One file of a kind it owns in, problems with codes out |
| `draft.apply` | Host, request | One finding in, draft file content out, by its drafting rule |
| `driver.prepare`, `driver.act`, `driver.health`, `driver.release` | Host, request | The driver interface; `driver.act` carries no token, which the host checks before sending |
| `discover.run` | Host, request | Run one discoverer within bounds; findings arrive as notifications |
| `discover.step` | Plug-in, request | The plug-in's only route to the system during discovery: one step through the sandbox port |
| `finding` | Plug-in, notification | One finding, checked against the manifest's kinds and level as it arrives |
| `describe.findings` | Host, request | The describer |
| `auth.sign_in`, `auth.is_signed_in`, `auth.detect_expiry`, `auth.refresh`, `auth.sign_out`, `auth.inject` | Host, request | The auth provider interface; called only on the keeper's behalf |
| `source.read`, `source.fingerprint` | Host, request | The source reader interface |
| `log`, `progress` | Plug-in, notification | Bounded and redacted by the host before storing; progress is a rising count, never a percentage |

The host supports the current and previous version. A method the plug-in does not know returns method-not-found and the capability is treated as absent; a message the host does not know closes the stream. The table in the technical reference is replaced by this one.

## Used by, and uses

| Caller | Uses |
| --- | --- |
| The product crate | `RegistryBuilder`, `Pack` |
| `a2m-engine` | `Registry::driver`, `DriverHandle`, `archetype`, `mode_defaults`, `source_reader`; implements `SandboxPort` |
| `a2m-actions` | `listing`, `discoverers_for` when it submits a discovery job, `auth_providers`, `compiler_set`, `action` for its dispatcher, `Roots::check` for the kernel's own reads, `supervisor` for `doctor.run` |
| A runner | `discoverers_for` and `Discoverer::discover`, inside the engine's sandbox, for a discovery job |
| Packs | `Pack`, `Registrar`, the traits, `PrepareContext`, `CheckedPath`, `ScopedSupervisor` to start their programs |
| `a2m-sched` | Nothing. Mode defaults reach it as data in a job's requirements |

It calls: `a2m-files` to accept compiler registrations; `a2m-telemetry` for spans; the pack and plug-in objects it holds.

## State

Memory only: the registry snapshot; for each subsystem its place, count, state, and owned group; cleanup states. The supervisor's ownership records are written through `a2m-ledger` from build step 7, so that a crashed engine's leftovers can be recognized at the next start. Settings choose which providers are enabled; they are read by the product crate, not here.

## Failures

| Class | Code | When |
| --- | --- | --- |
| `input` | `manifest_invalid`, `contract_unsupported`, `mode_conflict`, `path_not_allowed` | A manifest fails its checks; `Roots::check` refuses a path |
| `input` | `provider_ambiguous`, `mode_unknown` | Two ready providers and no setting; a mode nobody registered |
| `policy` | `effect_not_declared`, `signature_invalid`, `intent_missing` | A registration or start beyond the manifest; a bad signature; a `commits` step without a token |
| `busy` | `provider_unavailable`, `queue_full`, `cleanup_incomplete` | Not ready, with the availability state; a full subsystem queue; leftovers block a start |
| `unknown_variant` | `handshake_failed`, `undeclared_code`, `unexpected_absent` | A subsystem fails its handshake; a driver invents a code; `Absent` from a step that is no lookup |
| `transient` | `connection_lost`, `timeout` | A stream broke or a deadline passed on a step that commits nothing |
| `possible_commit` | `no_confirmation` | The same on a `commits` step |

## Invariants

| Must always hold | Proven by |
| --- | --- |
| A manifest that asks for an undeclared effect, or registers beyond it, is refused with a named failure | Package B3's test |
| A mode registered twice with two archetypes is refused | Package B3's test |
| Registration order never changes the registry's digest or any choice | A test that builds in every order |
| Two ready providers and no setting give `provider_ambiguous`, never a choice | Unit test |
| A `commits` step without a matching token never reaches a driver | A stub driver that fails the test if called |
| A doubtful write is always `possible_commit` | A stub driver returning `transient` with `Sent::Unknown` |
| An availability check starts no process | A test that counts child processes |
| A path outside its root, through `..` or a link, can never become a `CheckedPath` | `Roots::check` tests with planted links on three operating systems |
| A driver can start only what the manifest and the published plan declare | A pack stub that asks for an undeclared program and gets `effect_not_declared` |
| `for_reconcile` on a committing step is refused | A stub driver |
| A malformed or oversized frame closes the connection and fails every pending request | A test plug-in that misbehaves on cue |
| The supervisor never signals a process it cannot prove it started | A test that places a foreign process in reach; cleanup reports incomplete |
| No secret appears in a frame log, a capture, or a failure | The planted-secret test, extended to plug-in traffic |

## Build order

1. Manifest parsing and checks, `RegistryBuilder`, `Pack`, and a pack that registers only its modes. Package B3.
2. `Driver`, `DriverHandle`, `Authority`, and the choice rule, with a stub driver. Slice 1.1, with the web pack's interface driver as the first real one.
3. `CompilerRegistrations` gathered into `CompilerSet`. Slice 1.3.
4. The `commits` check against a real `IntentToken`. Slice 3.3.
5. `Discoverer`, `Describer`, `SandboxPort`. Package E1.
6. The supervisor for `Foreign` links, when the browser base first starts through it; `CheckedPath` with the first pack that opens a file; `OneShot` links with the command pack; then `Framed` links with the handshake.
7. Ownership records through `a2m-ledger`, and leftovers recognized at start.
8. The plug-in host and protocol version 1, with a test plug-in in another language. Package F3 for listing; Phase 8 for driving.

## Open points

- Whether an upstream MCP server's mapping file is its manifest. This design assumes so, listed under the `mcp` pack, and checked by that pack's file kind.
- Which operating-system mechanism backs `confined` on each platform. Until it is researched, a plug-in's effects are listed as `declared`.
- The widget driver extension point has no trait yet. First-party widget drivers stay inside the web pack's driver; the trait is written when a plug-in first needs to supply one.
- Reading another process's environment on macOS for the ownership check, and whether job-object membership is enough on Windows in every case.
- Whether `Posture::Discovery` and `Posture::Sandbox` should be one posture with a flag; they differ today only in whether test records exist.
