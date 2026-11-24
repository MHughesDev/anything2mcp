# `a2m-provider`

The one doorway between the kernel and everything that knows a kind of system. It holds the manifest and its checks, the registry that packs and plug-ins fill, the extension-point traits, the supervisor that starts and stops every outside process, and the plug-in host. It knows no kind of system itself.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 5, Contract |
| May depend on | Layers 1 to 4 |
| Depends on | `a2m-core`, including `CompiledTarget` and `Step` as data; `a2m-files` for the compiler registration traits and its schema type; `a2m-auth` for `SessionLease` and `AuthProvider`; `a2m-sources` for `SourceReader`; `a2m-ledger` for `IntentToken` and `RecordRef`; `a2m-telemetry` for the handle a pack is given; `a2m-netstate` for the machine's own interfaces, read to derive a place ([decision 0313](../../decisions/0313-the-place-of-an-address-is-derived-from-its-scope-and-the-machines-subnets.md)). Outside ([decision 0088](../../decisions/0088-the-provider-contracts-outside-crates.md)): `serde`, `toml`, `sha2`, `getrandom`, Tokio; `command-fds` and `rustix` on Linux and macOS; `windows-sys` on Windows. No JSON Schema validator. `rustix` also with its file-system calls, and `windows-sys` also with its thread snapshot ([decision 0148](../../decisions/0148-the-provider-contract-and-the-scheduler-after-their-safety-audit.md)) |
| Roadmap packages | Stage B, package B3 (manifest, registry); slice 1.1 (`Driver`); slice 1.3 (compiler registrations); Stage E, package E1 (`Discoverer`, `Describer`); package F3 and Phase 8 (plug-in host) |
| Carries out | Spec sections [202](../../spec/2-kernel/202-kernel-and-packs.md), [203](../../spec/2-kernel/203-the-provider-contract.md), [204](../../spec/2-kernel/204-supervising-subsystems.md), [218](../../spec/2-kernel/218-discovery-and-findings.md) for the discoverer's side, [213](../../spec/2-kernel/213-effects-hazards-and-consent.md) for effects; the [driver interface](../../reference/technical-reference.md#driver-interface) and the [plug-in protocol](../../reference/technical-reference.md#plug-in-protocol) |

## Owns

- The manifest: its file format, its schema, and the checks that a provider asks for nothing it did not declare.
- The `Registry`: one immutable snapshot of every provider, its modes with their archetypes, and the objects each supplied.
- The traits `Driver`, `Discoverer`, `Describer`, and `ProviderAction`, and the `DriverHandle` through which alone the engine reaches a driver.
- Traffic capture as a capability a pack may offer: `CaptureSource`, `TrafficCapture`, the form of what a capture yields (`CapturedTraffic`, `CapturedRequest`), and the check every yield passes (`take_capture`), with the registry's `CaptureHandle` ([decision 0391](../../decisions/0391-traffic-capture-is-a-capability-of-the-provider-contract-that-yields-structure-only.md)).
- The registered names a provider brings: modes with their archetypes, surface kinds, step kinds, file kinds, lint codes, finding kinds. Each is a checked name from `a2m-core`; the registry is the only list of them.
- The choice between two providers that could fill the same place, and its recorded reason.
- File access for packs: `CheckedPath`, which only the check against the allowed roots can make ([section 502](../../spec/5-security/502-defending-against-a-manipulated-agent.md), [section 213](../../spec/2-kernel/213-effects-hazards-and-consent.md)).
- The supervisor: the only way anything starts a process, with the handshake, the framing, deadlines, and ownership-proven stopping.
- The plug-in host, and the plug-in protocol's message list and version.
- The search for provider packages: `find_python_providers` reads the `anything2mcp.providers` entries of the installed packages in the site folders it is given, and each package's manifest, as text; it starts no Python and imports nothing, states where it looked (`EntryPointSearch::statement`), and gives each package it finds to `RegistryBuilder::add_plugin`, which lists it as present and not enabled. `PythonProvider::launch_spec` gives the supervisor the package's program, by its declared digest ([decision 0314](../../decisions/0314-a-provider-package-is-found-by-reading-its-entry-and-no-package-code-runs.md)).
- The availability check of section 203, which inspects the machine and starts nothing.
- The closed **shape** of a body, the module `shape`: the seven words of JSON Schema a target file may write a body's shape in (`type`, `nullable`, `properties`, `required`, `additionalProperties`, `items`, `enum`), its reading from a located document (`Shape::from_doc`), and one check of a value against it (`Shape::check`, through `ShapeValue`, giving a `Misfit` that holds no value). The HTTP pack's endpoint entries and the web pack's page requests read their shapes here; the browser base will check a page's body against it ([decision 0394](../../decisions/0394-an-asserted-read-of-a-web-surface-is-a-page-request-matched-by-one-shared-shape-and-shown-at-publish.md), point 1; [decision 0404](../../decisions/0404-one-closed-shape-of-a-body-in-the-provider-contract-and-the-stricter-refusal-where-the-two-copies-differed.md)).

It must never: decide whether something may happen; sign in or renew; write a ledger row; open the vault; retry a step; hold a list of modes or name a pack; let a caller's input choose a program, a file, or a destination.

## The manifest

One file per provider, `anything2mcp-provider.toml`. A first-party pack embeds the same file with `include_str!`, so packs and plug-ins pass one parser and one schema. TOML was chosen over YAML because a manifest is configuration a person reviews before enabling, read by machines on every start, and TOML has no anchors, aliases, or implicit typing; target files stay YAML, settings stay TOML ([decision 0014](../../decisions/0014-declarative-files-yaml-targets-toml-settings-cel-expressions.md)). The file's shape is checked by the types it is read into, `Manifest::parse`: a key the format does not know, or a badly formed name, is refused with the line. In a mode, `rank`, `intent_window`, `session_reads`, `session_writes`, and `exclusive` may be left out; they default to 100, 1, 1, 1, and `surface`.

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
commit_steps = ["save"]                  # the kinds that always save; the handle refuses one sent as a step that commits nothing. May be left out
absence_steps = []                       # the kinds whose "absent", after the settle time, is authoritative: only a lookup of these alone may prove that a record with no key did not land (decision 0185). Empty by default
rank = 40                                # default place in binding order when an operation states none; lower is tried first
intent_window = 1                        # intents stored ahead of acting, by default
session_reads = 4                        # parallel reads per session, by default
session_writes = 1                       # parallel writes per session, by default
exclusive = "file"                       # what one write holds alone: "none", "surface", "file", "screen", "link"

[[surface_kinds]]                        # registered names, like modes; a target's surface must name one that is registered
name = "file"
archetype = "artifact"
session = false                          # whether a session of the keeper is held on it

[[reads]]                                # kinds owned by another pack that this provider reads; the compiler hands it the validated entries as data
kind = "schemas"                         # a file kind or a finding kind
of = "file_kinds"                        # "file_kinds" | "finding_kinds"
surface_kind = "file"                    # the owner is the one pack that owns this kind for this surface kind

[points]                                 # the extension points it fills; nothing else can be registered
driver = ["sheet"]                       # modes it drives
discoverer = ["sheet_structure"]         # each discoverer also supplies its describer
auth_provider = []
source_reader = ["example_sheet_rows"]
widget_driver = []
traffic_capture = []                     # surface kinds whose traffic it captures, through one CaptureSource; empty: never asked (decision 0391)

[[discoverers]]
name = "sheet_structure"
archetype = "artifact"
kinds = ["schema", "field"]              # finding kinds it may return
sources = ["live_system", "artifact"]
max_level = "discovered"
bounds = { seconds = 60, bytes = 50000000, depth = 8 }
surface_kinds = ["file"]                 # the surface kinds it applies to; left out, any surface of its archetype
files = []                               # the names under which it reads files a person hands a discovery, from the project's `sources` folder; left out, none (decision 0263)

[[file_kinds]]
name = "sheets"
directory = "sheets/"
surface_kinds = ["file"]                 # the surface kinds it owns the kind for
schema = "schemas/sheets.json"           # plug-ins only: shipped beside the manifest. A pack registers the schema as an object

[[finding_kinds]]                        # the finding kinds it owns
name = "schema"
surface_kinds = ["file"]

[[controls]]                             # the controls it can operate
name = "grid"

[[failure_codes]]                        # every code its drivers may return, with its one class
class = "rejected"
code = "sheet_protected"

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
processes = []                           # programs it starts itself: [{ path = "...", digest = "sha256:..." }]; a pack names each by an absolute path
compiled_target_programs = false                    # true: it may also start programs a published compiled target names, with their digests
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

[[settings]]                             # the pack's own group, `packs.example_sheet`; one entry for each key
key = "max_rows"
type = "int"                             # "bool" | "int" | "text" | "choice" | "list"
default = 100000
min = 1                                  # for "int"; `choices = [...]` for "choice"
max = 1000000
scope = "user"                           # "installation" | "user" | "project"
protected = false                        # a protected setting also states `safer = "lower"`
doc = "The most rows one read may return."
```

A kind has one owning pack per surface kind, and another pack may declare that it reads it (decision 0062). The web pack's manifest, for instance, lets its `page_http` mode use the HTTP pack's endpoint catalog without owning it:

```toml
# In the web pack's manifest
[[reads]]
kind = "endpoints"                       # owned by the HTTP pack
of = "file_kinds"
surface_kind = "web"
```

Checks made when a manifest is read, and when the registry is built, each a named failure:

| Check | Failure |
| --- | --- |
| The file parses, has no key the format does not know, and every name in it is well formed | `input`, `manifest_invalid`, with the line; a rule that spans fields names the field |
| `contract` is current or previous | `input`, `contract_unsupported` |
| A mode or surface kind is stated once in a manifest. Two providers that register one mode agree on its archetype. A surface kind has one provider | `input`, `mode_conflict`, for each provider involved |
| Every step kind, file kind, lint code, finding kind, control, and failure code is listed once; a lint code and an action start with the provider's name; a step kind that always commits is one of the mode's step kinds | `input`, `manifest_invalid` |
| A failure code of the provider contract keeps its own class, and no code is one of the kernel's own, `codes::KERNEL_ONLY`: `blocked_by_parent`, `runner_lost`, `cannot_reconcile`, `reconcile_required`, `approval_missing`, `approval_stale`, `lease_superseded`. These state the kernel's bookkeeping, and some of the kernel's logic goes by the code alone | `input`, `manifest_invalid` |
| Every kind a provider `reads` has exactly one enabled owner for that surface kind. If the owner is absent or disabled, the reader stays registered, the problem is listed, and any binding that needs the kind does not compile | `input`, `kind_owner_missing`, naming the kind, the surface kind, and the pack to enable |
| No two enabled packs own one kind for one surface kind | `input`, `kind_owner_conflict` |
| A point names only a declared mode or discoverer; a surface kind that holds a session, and an auth provider, are covered by `effects.sessions`; a discoverer returns only finding kinds the manifest owns or reads. Every object registered matches what is declared | `policy`, `effect_not_declared` |
| A plug-in's signature verifies, or unsigned plug-ins are switched on by a person. Not built: a plug-in is listed as present and not enabled | `policy`, `signature_invalid` |
| A discoverer's `max_level` is `inferred` or `discovered` | `input`, `manifest_invalid` |
| A discoverer's `files` are names of lowercase letters, digits, and underscores, starting with a letter, at most 32 bytes, each listed once | `input`, `manifest_invalid` |
| A program in `run` or `effects.processes` is a relative path inside the provider or a fixed absolute path, and a requirement is looked for only at fixed absolute paths. A pack has no folder of its own, so its `effects.processes` are absolute paths only | `input`, `path_not_allowed` |
| A pack's settings group fits the `[[settings]]` its manifest declares | `input`, `pack_settings_invalid`; the pack is `misconfigured` and registers nothing |
| Two providers carry one name | `input`, `provider_duplicate` |

## Interface

This is the interface as it is built ([decision 0089](../../decisions/0089-the-provider-contract-as-building-it-settled-it.md)). Every async method of every trait here returns `BoxFuture<'a, T>`, which is `Pin<Box<dyn Future<Output = T> + Send + 'a>>` and the same type `a2m-auth` uses, so each trait is a trait object. An implementation writes `Box::pin(async move { ... })`.

```rust
// The registry. Built once at start by the product crate; replaced whole when a provider is enabled or disabled.
pub struct HostServices { pub supervisor: Supervisor, pub roots: Roots, pub redact: Arc<dyn Redact>, pub telemetry: Telemetry }
pub struct ProviderSettings {
    pub disabled: BTreeSet<ProviderName>,                 // a compiled-in pack is enabled unless listed
    pub mode_providers: BTreeMap<Mode, ProviderName>,     // which provider drives a contested mode
    pub packs: BTreeMap<ProviderName, Values>,            // each pack's own group, `packs.<pack>`, as written
}
pub struct RegistryBuilder { /* settings, host services, collected providers */ }
impl RegistryBuilder {
    pub fn new(settings: ProviderSettings, host: HostServices) -> Self;
    /// One call per first-party pack. The product crate makes no other call per pack.
    pub fn add_pack(&mut self, pack: &dyn Pack);
    /// A plug-in found on the machine. Listed as present and not enabled; it registers nothing.
    pub fn add_plugin(&mut self, found: &FoundManifest);
    /// Every problem at once. A provider with a problem is listed with it and registers nothing,
    /// except one that reads a kind whose owner is missing (`registered: true`).
    pub fn build(self) -> (Registry, Vec<RegistryProblem>);
}

pub trait Pack: Send + Sync {
    fn manifest(&self) -> &'static str;                        // the TOML text, embedded with include_str!
    fn register(&self, cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure>;
}

/// What a pack is handed when it registers. Clone it into the objects that need it.
pub struct PackContext { /* private */ }
impl PackContext {
    pub fn provider(&self) -> &ProviderName;
    pub fn supervisor(&self) -> &ScopedSupervisor;             // starts only what the manifest declares
    pub fn roots(&self) -> &Roots;                             // only the roots the manifest declares
    pub fn folder(&self, root: &RootName, access: Access) -> Result<CheckedPath, Failure>;
    pub fn redact(&self) -> &Arc<dyn Redact>;
    pub fn telemetry(&self) -> &Telemetry;
    pub fn settings(&self) -> &Values;                         // `packs.<pack>`, validated, defaults filled in
}

/// What a pack hands over. Each call is checked against the manifest as it is made;
/// anything undeclared, or handed over twice, is `policy`, `effect_not_declared`.
pub struct Registrar<'a> { /* the provider being registered */ }
impl Registrar<'_> {
    pub fn driver(&mut self, mode: Mode, driver: Arc<dyn Driver>) -> Result<(), Failure>;
    pub fn discoverer(&mut self, d: Arc<dyn Discoverer>, describer: Arc<dyn Describer>) -> Result<(), Failure>;
    pub fn auth_provider(&mut self, p: Arc<dyn AuthProvider>) -> Result<(), Failure>;   // trait from `a2m-auth`
    pub fn source_reader(&mut self, r: Arc<dyn SourceReader>) -> Result<(), Failure>;   // trait from `a2m-sources`
    pub fn compiler(&mut self, c: CompilerRegistrations) -> Result<(), Failure>;        // may be called more than once
    pub fn action(&mut self, a: Arc<dyn ProviderAction>) -> Result<(), Failure>;
}                                                              // modes, step kinds, and finding kinds come from the manifest alone

/// The objects a pack brings to the compiler; the traits are `a2m-files`'s.
#[derive(Clone, Default)]
pub struct CompilerRegistrations {
    pub surface_kinds: Vec<SurfaceKindSpec>, pub controls: Vec<ControlName>,
    pub file_kinds: Vec<Arc<dyn FileKind>>, pub lowerings: Vec<Arc<dyn Lowering>>,
    pub lint_rules: Vec<Arc<dyn LintRule>>, pub drafting_rules: Vec<Arc<dyn DraftingRule>>,
}                                                              // with a builder method of the same name for each
// Each lowering states, for every step kind it emits, whether a step of it can change the system
// (`Lowering::changes`, a `StepChanges`); a kind it leaves unmarked counts as one that can, and the
// compiler refuses a read that holds one (decision 0307, point 4). The manifest's `steps` list stays as it is.

/// An action a provider adds. Its faces are generated by `a2m-actions`, whose dispatcher alone calls it,
/// after the caller's credential is checked and policy has decided. It gets checked paths and the registry, never the store.
pub trait ProviderAction: Send + Sync {
    fn name(&self) -> ActionName;                                      // as declared in the manifest
    fn input_schema(&self) -> &JsonSchema;                             // `a2m-files`'s schema type
    fn output_schema(&self) -> &JsonSchema;
    fn handle<'a>(&'a self, input: &'a Values, cx: &'a ActionContext<'a>) -> BoxFuture<'a, Result<Values, Failure>>;
}
pub struct ActionContext<'a> { pub roots: &'a Roots, pub registry: &'a Registry, pub redact: &'a dyn Redact, pub paths: &'a CheckedPaths }

#[derive(Clone)]
pub struct Registry { /* immutable; an Arc inside */ }
impl Registry {
    pub fn digest(&self) -> Digest;                            // keys the tool-list cache and is recorded in every trace
    pub fn archetype(&self, mode: &Mode) -> Option<Archetype>;
    pub fn modes(&self) -> impl Iterator<Item = &Mode>;
    pub fn surface_kind(&self, kind: &SurfaceKind) -> Option<&SurfaceKindEntry>;   // archetype, whether it holds a session, which provider
    pub fn mode_defaults(&self, mode: &Mode) -> Option<ModeDefaults>;   // rank, intent window, session limits, exclusive
    /// The driver for a mode, with the provider choice already made and its reason attached.
    pub fn driver(&self, mode: &Mode) -> Result<DriverHandle, Failure>;
    pub fn source_reader(&self, name: &ReaderName) -> Result<Arc<dyn SourceReader>, Failure>;   // those packs registered; the built-in ones are `a2m-sources`'s
    pub fn source_readers(&self) -> Vec<Arc<dyn SourceReader>>;
    pub fn discoverers_for(&self, surface: &SurfaceDescription) -> Vec<DiscovererHandle>;
    pub fn files_taken(&self, surface: &SurfaceDescription) -> BTreeSet<String>;   // the `files` those discoverers declare: the names a discovery of the surface may be handed
    pub fn auth_providers(&self) -> Vec<Arc<dyn AuthProvider>>;          // handed to the keeper by `a2m-actions`
    pub fn compiler_set(&self) -> &CompilerSet;                         // handed to the compiler by `a2m-actions`
    pub fn action(&self, name: &ActionName) -> Option<(Arc<dyn ProviderAction>, EffectClass)>;   // for the dispatcher in `a2m-actions`
    pub fn actions(&self) -> impl Iterator<Item = &ActionName>;
    pub fn kind_owner(&self, of: KindOf, kind: &str, surface_kind: &SurfaceKind) -> Option<&ProviderName>;
    pub fn listing(&self) -> Vec<ProviderListing>;                      // manifest, availability, problems; for `provider.list`
    pub fn provider_roots(&self, provider: &ProviderName) -> Option<&Roots>;
    pub fn provider_settings(&self, provider: &ProviderName) -> Option<&Values>;
    pub fn supervisor(&self) -> &Supervisor;
    pub fn roots(&self) -> &Roots;                                      // every named root, for the kernel's own reads
}
```

**There is no priority and no silent fallback.** If one mode is driven by two ready providers and no setting names one, `driver` fails with `input`, `provider_ambiguous`, and the message lists each candidate with its availability. If the named one is not ready, it fails with `busy`, `provider_unavailable`, carrying the availability state and its fix; it never falls to the other. Registration order never affects the result, and a test builds the registry in every order and compares digests.

**The digest** covers, for every registered provider in order of name, its version, the digest of its manifest, its availability, the names of every object it registered, and the digest of its validated settings; and every choice a setting makes. A provider that is listed and not registered is not in it.

**Declared against observed.** `ProviderListing` carries the manifest (what it can do), the availability (can it here, now), whether its objects are registered, and its problems. Availability is computed when the pack is added, by looking for each requirement at its declared paths and taking its digest, and by comparing the host with `platforms`. A declared path that is not absolute on this operating system, such as another system's form, is passed over: it would be looked for in the engine's own working folder. The availability is a report; the digests are checked again, by the supervisor, at every start. It starts no process: the function has no way to reach the supervisor. Its states are the eight of section 203. A requirement's `versions` is text for a person; the check cannot ask a program its version without starting it. Health, as a third answer, is `DriverHandle::health`.

### The extension points

The driver interface is settled here as follows, closing the two points the technical reference leaves to slice 1.1.

1. **Confirm and read-back are lists of steps, carried out by `act`.** There is no `read_back` method. The compiler lowers a binding into its step lists, and the engine passes each step to `act`. The compiled target then says everything a driver will do, the trace has one record per step whatever the part of the loop, and the preview, the freshness check, and reconcile run the read-back list on its own exactly as file formats require. A separate method would let a pack hide reading logic the compiled target does not show, and would give two ways to read a record.
2. **A step can report a record as absent.** `StepOutcome::Absent` is allowed only for a step the compiled target marks `lookup`. Anywhere else it is refused by the handle as `unknown_variant`, `unexpected_absent`.
3. **Declared capabilities leave the trait.** The sketch's `mode()` and `capabilities()` repeat the manifest; knowledge lives in one place, so the registry answers both.
4. **Every failure says whether anything was sent,** as the supervisor's rule asks of every subsystem.
5. **A step can return what reconcile will need before the commit point.** A key a store assigned inside an open transaction, or the digest a file will have once it is replaced, is learned before the step that commits. A step not marked `commits` may return it as `Observed::for_reconcile`; the engine records it durably before the committing step is sent. The handle refuses `for_reconcile` on the committing step itself, as `unknown_variant`, `reconcile_too_late`.

```rust
pub trait Driver: Send + Sync {
    /// Acquire what the binding needs: a tab, a client, a window, an open file. `lease` is None for a surface with no session.
    fn prepare<'a>(&'a self, cx: &'a PrepareContext<'a>, lease: Option<&'a SessionLease>)
        -> BoxFuture<'a, Result<Prepared, StepFailure>>;
    /// Carry out exactly one step of the kinds this mode declared. Never retries. Values are already evaluated.
    fn act<'a>(&'a self, prepared: &'a mut Prepared, step: &'a Step, input: &'a StepInput<'a>)
        -> BoxFuture<'a, Result<StepOutcome, StepFailure>>;
    /// From memory, without input or output: may this driver be chosen for this surface now.
    fn health(&self, surface: &SurfaceRef) -> DriverHealth;
    /// Give back what prepare acquired. `poisoned` after a failure: recycle, never reuse.
    fn release<'a>(&'a self, prepared: Prepared, poisoned: bool) -> BoxFuture<'a, Cleanup>;
}

pub struct Prepared { /* the driver's own state; opaque to the kernel */ }
impl Prepared {
    pub fn new<T: Any + Send>(state: T) -> Self;
    pub fn state<T: Any>(&mut self) -> Option<&mut T>;
    pub fn into_state<T: Any>(self) -> Result<Box<T>, Self>;
}
pub struct PrepareContext<'a> {
    pub run: RunId, pub epoch: Epoch,                         // the run and its lease number
    pub surface: &'a SurfaceRef,
    pub compiled_target: &'a CompiledTarget,                                       // the published compiled target as data; a step's `refers_to` names an entry of a pack section
    pub posture: Posture, pub deadline: Deadline, pub redact: &'a dyn Redact,
    pub destinations: &'a Destinations,                       // the target's declared surfaces, within the manifest's network effect
    pub paths: &'a CheckedPaths,                              // every file this run may touch, already checked; a driver resolves none;
                                                              // `paths.project()` / `project_folder()`: the project folder of the request that started the run
    pub supervisor: &'a ScopedSupervisor,                     // starts only what the manifest and the published compiled target declare
}
pub struct SurfaceRef { pub target: TargetName, pub environment: Environment, pub surface: CompiledSurface }   // at this environment's address
pub enum Posture { Live, Sandbox { test_records: bool }, Discovery }   // the driver applies its own kind's read-only guard outside Live
pub struct Deadline(/* a Tokio instant */);                   // Deadline::after(duration), at(), remaining(), passed()
pub struct Destinations { /* addresses, compared as written, each with its scope */ }  // Destinations::new(addresses), contains(address), iter(), scope_of(address) -> Option<&ScopedHost>, place_of(address, &MachineState) -> Option<Placed>
/// Where an address is and how this machine reaches it (decision 0313): derived from the scope and the machine's state, never an input.
pub enum NetworkPlace { ThisMachine, LocalNetwork, PublicInternet, ViaGateway /* never derived yet */, Unknown }
impl NetworkPlace { pub fn of(host: &ScopedHost, state: &MachineState) -> Placed; pub fn is_direct(self) -> bool; pub fn as_str(self) -> &'static str; }
pub struct Placed { pub place: NetworkPlace, pub because: &'static str }
/// The subnets of the interfaces that are up. `read()` asks `a2m-netstate`; a reading that fails is `unread()`: every place but `this_machine` is `unknown`.
pub struct MachineState; // read(), of(&Interfaces), of_subnets(..), unread()
pub struct StepInput<'a> {
    pub args: &'a Values,                                     // the step's arguments, by name, already evaluated
    pub record: Option<RecordRef>,                            // the commit point the step belongs to (`a2m-ledger`)
    pub key: Option<&'a RecordKey>,                           // the record's key in the system; never its place in a run (decision 0161)
    pub stamp: Option<&'a str>,                               // the record's idempotency token
    pub deadline: Deadline,
}                                                             // StepInput::new(args, deadline).for_record(record, key, stamp)
// What the record's earlier steps returned in `for_reconcile`, for the step being acted: `Prepared::recorded()`,
// set by the handle (`DriverHandle::act_recorded`) for the one call of `act` and empty outside it (decision 0162).
// Whether another record of the operation, in any run of the workspace, holds a recorded value (decision 0193):
// Prepared::recorded_elsewhere(name, &value, deadline) -> RecordedElsewhere { Held(RecordRef), NotHeld, CannotTell },
// compared by recorded_digest(value), never handed back; answered by an OtherRecords the engine passes to
// DriverHandle::act_asking for one step; CannotTell without one, outside act, for a malformed name, a null, a list,
// a record, a masked value, a late answer, or an answer that names the asking record.
pub enum StepOutcome { Done(Observed), Absent(Observed) }
#[derive(Default)]
pub struct Observed {
    pub values: Values, pub assigned_key: Option<RecordKey>,
    pub for_reconcile: Values,                                // learned before the commit point; recorded before the committing step
    pub captures: Vec<Capture>,                               // already redacted
}
pub struct Capture { pub name: String, pub media_type: String, pub bytes: Vec<u8> }
// Capture::acting(&Acting): the capture named `acting`, of `application/json`, says how the step was done; the engine puts it in
// the step's trace record (`means` in the signature, `measured` not) and keeps it as no evidence. Observed::acting() reads it.
pub struct StepFailure { pub failure: Failure, pub sent: Sent }       // StepFailure::not_sent(f), answered(f), unknown(f);
                                                                      // .with_said(text): what the system said, kept apart, untrusted
pub enum Sent { NotSent, Answered, Unknown }                   // Answered: the system replied, so the outcome is known
pub enum DriverHealth { Ready, Degraded(FailureCode), Unavailable(FailureCode) }

/// File access. A `CheckedPath` is made only by `Roots::check`, `Roots::folder`, and `CheckedPath::join`.
pub struct Roots { /* named roots, each readable or writable */ }
impl Roots {
    pub fn none() -> Self;
    pub fn add(&mut self, name: RootName, folder: &Path, access: Access) -> Result<(), Failure>;   // an existing folder, by absolute path
    pub fn narrowed(&self, read: &[RootName], write: &[RootName]) -> Result<Self, Failure>;        // what one manifest declares
    pub fn folder(&self, root: &RootName, access: Access) -> Result<CheckedPath, Failure>;
    pub fn check(&self, root: &RootName, relative: &str, access: Access) -> Result<CheckedPath, Failure>;
    // A root of each request's project, bound to no folder at registry time (decision 0193):
    pub fn add_project(&mut self, name: RootName, access: Access) -> Result<(), Failure>;
    pub fn check_in(&self, root: &RootName, relative: &str, access: Access, paths: &CheckedPaths) -> Result<CheckedPath, Failure>;
    pub fn folder_in(&self, root: &RootName, access: Access, paths: &CheckedPaths) -> Result<CheckedPath, Failure>;
    pub fn holds(&self, root: &RootName, access: Access) -> bool;   pub fn is_project(&self, root: &RootName) -> bool;
    // check and folder refuse such a root with input/project_folder_missing; check_in resolves it in paths.project_folder()
}
/// A path that exists, every link followed, in the one form every root holds its folder in: on Windows the
/// ordinary form `C:\...` where it names the same thing, else the verbatim one; elsewhere as `canonicalize` gives.
/// Whatever compares a path with a root's folder resolves it by this, and by nothing else.
pub fn resolved_path(path: &Path) -> std::io::Result<PathBuf>;
pub enum Access { Read, Write }
pub enum EntryKind { Absent, File, Folder, Link, Other }
#[derive(Clone)]
pub struct CheckedPath { /* private; no public constructor */ }
impl CheckedPath {
    pub fn root(&self) -> &RootName;   pub fn relative(&self) -> &str;   pub fn access(&self) -> Access;
    pub fn as_path(&self) -> &Path;                           // to hand to a program or a system call; never to build a path from
    pub fn join(&self, relative: &str) -> Result<Self, Failure>;
    pub fn exists(&self) -> bool;   pub fn is_dir(&self) -> bool;   pub fn size(&self) -> Result<u64, Failure>;
    pub fn entry_kind(&self) -> Result<EntryKind, Failure>;             // what stands at the final name now; a link is reported, never followed
    pub fn open(&self) -> Result<std::fs::File, Failure>;
    pub fn read(&self, max_bytes: u64) -> Result<Vec<u8>, Failure>;
    pub fn write(&self, bytes: &[u8]) -> Result<(), Failure>;           // to a new file of a random name in the same folder, which then takes the name; never through a link
    pub fn create_dir(&self) -> Result<(), Failure>;                    // this one folder, new: fails when anything stands at the name
    pub fn create_dir_all(&self) -> Result<(), Failure>;
    pub fn remove_file(&self) -> Result<(), Failure>;                   // never through a link
    pub fn list(&self) -> Result<(Vec<Self>, usize), Failure>;          // entries in order of name, and how many were left out
}
pub struct CheckedPaths { /* by the name the compiled target gives each */ }       // none(), insert(name, path), get(name), iter()

/// The only way the engine reaches a driver. Not constructible outside this crate.
#[derive(Clone)]
pub struct DriverHandle { /* the driver, the mode's declared steps and codes, the provider choice */ }
impl DriverHandle {
    pub fn mode(&self) -> &Mode;   pub fn archetype(&self) -> Archetype;
    pub fn choice(&self) -> &ProviderChoice;                   // provider, version, manifest digest, reason: recorded in the trace
    /// The supervisor for the PrepareContext: the manifest's programs, and the compiled target's where the manifest says `compiled_target_programs`.
    pub fn supervisor(&self, compiled_target_programs: Vec<ProgramRef>) -> ScopedSupervisor;
    pub async fn prepare(&self, cx: &PrepareContext<'_>, lease: Option<&SessionLease>) -> Result<Prepared, StepFailure>;
    /// A step marked `commits` needs `Authority::Write` with the token `a2m-ledger` issued for its record, or it is refused unsent.
    pub async fn act(&self, prepared: &mut Prepared, step: &Step, input: &StepInput<'_>, authority: Authority<'_>)
        -> Result<StepOutcome, StepFailure>;
    pub fn health(&self, surface: &SurfaceRef) -> DriverHealth;
    pub async fn release(&self, prepared: Prepared, poisoned: bool) -> Cleanup;
}
pub enum Authority<'a> { Read, Write(&'a IntentToken) }

pub trait Discoverer: Send + Sync {
    fn name(&self) -> DiscovererName;                          // as the manifest declares it
    /// From the surface description alone: does it apply here.
    fn suits(&self, surface: &SurfaceDescription) -> bool;
    /// Observe within the scope's bounds, reaching the system only through `port`. Findings go to `out` as they are made.
    fn discover<'a>(&'a self, port: &'a dyn SandboxPort, scope: &'a DiscoveryScope<'a>, out: &'a mut dyn FindingSink)
        -> BoxFuture<'a, Result<DiscoveryReport, Failure>>;
}
pub struct SurfaceDescription { pub kind: SurfaceKind, pub archetype: Archetype, pub address: String, pub facts: Values }
pub struct DiscoveryScope<'a> {
    pub surface: &'a SurfaceRef, pub bounds: Bounds, pub deadline: Deadline,
    pub paths: &'a CheckedPaths, pub redact: &'a dyn Redact, pub focus: &'a Values,
}
pub trait FindingSink: Send { fn found(&mut self, finding: Finding) -> Result<(), Failure>; }
#[derive(Default)]
pub struct DiscoveryReport { pub coverage: Coverage, pub bounds_hit: Vec<String>, pub dropped: u64 }

/// Implemented by the engine's sandbox; a write cannot pass through it. `ReadOnlyPort::new(handle, prepared)` is one, ready made.
pub trait SandboxPort: Send + Sync {
    fn act<'a>(&'a self, step: &'a Step, input: &'a StepInput<'a>) -> BoxFuture<'a, Result<StepOutcome, StepFailure>>;
}

pub trait Describer: Send + Sync {
    /// Pure: findings in, text and structure an agent can read out. No input or output; untrusted text stays apart.
    fn describe(&self, findings: &[Finding], form: DescribeForm) -> Description;
}
pub enum DescribeForm { Summary, Full }
#[derive(Default)]
pub struct Description { pub text: String, pub data: Values, pub untrusted: Vec<String> }

/// Traffic capture (decision 0391). A pack that lists kinds under `points.traffic_capture` registers one source
/// with `Registrar::traffic_capture`; one that lists none and registers one is refused, `effect_not_declared`.
pub trait CaptureSource: Send + Sync {
    fn open<'a>(&'a self, surface: &'a SurfaceRef, deadline: Deadline) -> BoxFuture<'a, Result<Box<dyn TrafficCapture>, Failure>>;
}
pub trait TrafficCapture: Send + Sync {
    fn start(&self);
    fn take_recorded<'a>(&'a self, until: Deadline) -> BoxFuture<'a, CapturedTraffic>;   // read only through take_capture
}
pub struct CapturedTraffic { pub requests: Vec<CapturedRequest>, pub dropped: usize, pub awaited: usize, pub withheld: usize }
pub struct CapturedRequest {   // structure only: no field holds a value of a query, a header, a cookie, or a body
    pub method: String, pub origin: String, pub path: String, pub query: Vec<String>, pub query_left_out: bool,
    pub credential_headers: Vec<String>, pub set_headers: Vec<String>, pub headers_left_out: bool,
    pub status: u16, pub media_type: String, pub fields: Option<BTreeMap<String, String>>, pub names_withheld: bool,
}
pub async fn take_capture(capture: &dyn TrafficCapture, until: Deadline, redact: &dyn Redact) -> CapturedTraffic;
impl Registry { pub fn captures_for(&self, kind: &SurfaceKind) -> Vec<CaptureHandle>; }   // only declaring packs
impl CaptureHandle { pub async fn open(&self, surface: &SurfaceRef, deadline: Deadline) -> Result<Capturing, Failure>; }
impl Capturing { pub async fn take(&self, until: Deadline) -> CapturedTraffic; }          // take_capture, the host's redactor

/// A registered discoverer, as the registry gives it to a runner.
impl DiscovererHandle {
    pub fn provider(&self) -> &ProviderName;   pub fn declared(&self) -> &DiscovererDecl;
    pub fn suits(&self, surface: &SurfaceDescription) -> bool;
    pub async fn discover(&self, port: &dyn SandboxPort, scope: &DiscoveryScope<'_>, out: &mut dyn FindingSink)
        -> Result<DiscoveryReport, Failure>;                   // each finding is checked against the manifest as it arrives
    pub fn describe(&self, findings: &[Finding], form: DescribeForm) -> Description;
}
```

Contracts the signatures do not show:

- **The handle is the enforcement point.** Before calling the pack it checks: the step's kind is one the mode declared (`unknown_variant`, `step_kind_unknown`); a `commits` step is asked for in a posture that may write, `Live` or `Sandbox` with test records (`policy`, `read_only_posture`), and carries a token whose run and lease number are those `prepare` was given and whose record is the one in the step's input (`policy`, `intent_missing`). A refused step is reported `Sent::NotSent`. A `Prepared` that did not come from the handle's own `prepare` commits nothing. Where a mode's manifest lists `commit_steps`, a step of such a kind that is not marked `commits` is refused too, `policy`, `intent_missing`: the mark a compiled target or a discoverer puts on a step is then not all that stands between a read and a write.
- **The handle fixes the class of a write that was sent and is not settled.** A failure on a `commits` step with `Sent::Unknown` becomes `possible_commit`, `no_confirmation`, whatever class the driver gave. A driver cannot make a doubtful write look retryable. So does a failure on a `commits` step with `Sent::Answered` whose class is `policy` or `input`, and it leaves the handle as `Sent::Unknown`: those two classes say that nothing was sent, the ledger releases a record's lock on them without reading the record, and on a write that did leave they are not the system's verdict. `stale` stays, because there the system answered that the record had changed and nothing was written; `rejected` and every other class stay. A failure with `Sent::NotSent` keeps its class.
- **A failure's class is one of the twelve, and its code is declared.** A code passes when the manifest's `failure_codes` lists it with the same class, when it is the step's own `fails_as`, or when it is a code of this crate or of the session keeper. Anything else is refused as `unknown_variant`, `undeclared_code`, and so is a code of `codes::KERNEL_ONLY` whatever declares it. Every failure passes the redactor on its way out of the handle.
- **Cancellation.** A driver must be safe to cancel on any step not marked `commits`. On a `commits` step the engine waits to the deadline and never drops the future.
- **No secret leaves a driver.** Credentials arrive through `SessionLease::apply`, which writes into a sink the driver owns and never returns the value. Captures pass the `Redact` the context carries before they leave `act`. The handle is the backstop: on the way out of `act` it redacts every text in `Observed::values` and `for_reconcile`, a capture's name, and a capture whose bytes are text. Bytes that are not text cannot be searched and pass as the driver left them. A value that was redacted no longer equals what was intended, so a read-back that returns a secret ends as a mismatch and not as a stored secret.
- **There is one redactor.** `PackContext::redact`, `PrepareContext::redact`, and the handle's own are the same object: the vault's redactor, which the action registry's kernel hands to `HostServices` and to the engine alike. `PrepareContext::redact` is a borrow that ends with `prepare`, and `StepInput` carries none, so a driver redacts in `act` with the one it keeps from its `PackContext`. A host that built the two from different objects would break this; nothing checks it.
- **What a capture yields is structure, and passes the contract's check whatever the pack did.** `take_capture` withholds whole a request whose method or origin is not of its form or holds a secret the redactor knows, and counts it; cuts a query or fragment off a path; and holds every path segment, every name of a query's parameter or a header, and each part of a field's name to one predicate, `is_captured_word` (at most 32 characters of ASCII letters, digits, `_`, `-`, `.`; at most three digits; no run of more than 15 hexadecimal digits) and to the redactor. A segment that fails is `{value}`; a name that fails is dropped, saying one was left out. It holds by itself, whatever the pack flagged. A surface of a kind the manifest does not declare is refused by the handle before the pack is asked (decision 0391).
- **A discoverer has no handle and no token.** `discover` is given a `SandboxPort`, a scope, and a sink. The port's one method takes no `Authority`, so there is nowhere to pass a token; a `DriverHandle` comes only from the registry, which no pack is given; an `IntentToken` has no public constructor. `ReadOnlyPort` sends every step with `Authority::Read`, so a step marked `commits` is refused before the driver sees it. For a first-party pack that is structural and reviewed; for a plug-in it is only as strong as the effects listing says. What a read-only step is, for its archetype, each pack states and its conformance test proves. The limit of this: a discoverer builds its own steps, and the driver behind the port may be another pack's, so a step of a kind that saves, sent without the `commits` mark, is stopped by that driver's own posture guard and by nothing in the kernel, unless the mode lists the kind under `commit_steps`. No first-party manifest lists one yet.
- **`CheckedPath` refuses the same forms on every operating system:** an empty path, an absolute path, `.` and `..`, a backslash, a colon (so every drive, stream, and network form), a home folder mark, a control character, a part that ends with a dot or a space, a device name, and a link that leads outside the root. A link that stays inside is followed. The path is resolved once; every method that opens the file looks once more that it still resolves inside the root.
- **What `check` and `join` promise about links.** Every link that exists in the path when it is made is followed, in a folder on the way and at the final name alike, as long as it leads inside the root; one that leads outside, or nowhere, is refused. So the path a `CheckedPath` holds, which `as_path` gives and the reading methods use, has no link in it, and a pack that looks at it cannot tell a file from a link to one. `entry_kind` is how it can: it reports what stands at the final name as written, `Absent`, `File`, `Folder`, `Link`, or `Other`, and follows nothing.
- **A write and a removal never go through a link.** `write` and `remove_file` act on the final name in its folder. Where a link stands at that name they refuse, `input`, `path_not_allowed`, whether the link was there when the path was made or was put there since. `write` makes its new file under a name of 128 random bits from the operating system, only if nothing stands at that name, and without following a link; the name the file then takes is replaced and never followed. `create_dir` makes one folder and fails when anything is at the name.
- **The moment of use.** On Linux and macOS `write`, `remove_file`, `create_dir`, `open`, `read`, and `entry_kind` reach the folder by opening each folder from the root down without following a link, and act on the name relative to that handle. A link put on the way after the check is refused, also one that stays inside the root, and nothing done to the path meanwhile leads the work elsewhere. On Windows, where the standard library cannot open a name relative to a folder's handle, `open` and `read` hold every folder from the root down while they open, each opened with no link followed and not shared for removal, so that none of them can be moved, removed, or replaced while the file is opened, and they refuse a link at the name; a `HeldFolder` holds the same chain for as long as it lives, and each of its calls names the entry by its path through the held folders ([decision 0272](../../decisions/0272-the-file-packs-folder-writes-act-through-a-held-folder-after-a-second-reader.md)). The other methods (`exists`, `is_dir`, `size`, `list`, `create_dir_all`), and on Windows also `write`, `remove_file`, `create_dir`, and `entry_kind`, look again that the path resolves inside the root and then use it by name: a link swapped in between those two moments is followed. Making one needs the right to write in a folder of the root. `as_path` handed to a program is outside all of this.

### The shape of a body

```rust
pub mod shape {
    pub enum Kind { Any, Null, Boolean, Integer, Number, String, Array, Object }   // the words of `type`
    impl Kind { pub const WORDS: &[&str]; pub fn from_word(&str) -> Option<Kind>; pub const fn word(self) -> &'static str; }
    pub struct Shape {                                    // serialized into a pack's payload as written here
        pub kind: Kind, pub nullable: bool,
        pub properties: BTreeMap<String, Shape>, pub required: Vec<String>,
        pub closed: bool,                                 // `additionalProperties: false`
        pub items: Option<Box<Shape>>, pub one_of: Vec<String>,   // `enum`, for a string
    }
    impl Shape {
        pub fn of(kind: Kind) -> Shape;
        pub fn from_doc(doc: &Located<Doc>) -> Result<Shape, ShapeProblem>;   // the first problem, at its place
        pub fn at_path(&self, path: &[String]) -> Option<(&Shape, bool)>;     // `*` steps into items; whether certain
        pub fn check<V: ShapeValue>(&self, value: &V) -> Result<(), Misfit>;
    }
    pub struct ShapeProblem { pub at: Place, pub message: String }
    pub trait ShapeValue { fn view(&self) -> ValueView<'_, Self>; }          // implemented for serde_json::Value
    pub enum ValueView<'a, V> { Null, Boolean, Number(Cow<'a, str>), Text(&'a str), List(&'a [V]), Object(Vec<(&'a str, &'a V)>) }
    pub struct Misfit { pub path: Vec<PathStep>, pub kind: MisfitKind }      // never the value
    pub enum PathStep { Name(String), Item(usize) }
    pub enum MisfitKind { Kind { found: &'static str, declared: Kind }, NotOneOf, Lacks(String), Undeclared }
    impl Misfit { pub fn path_from(&self, root: &str) -> String; }            // `the body.lines[1].n`
}
```

A word other than the seven, or one the shape's `type` does not take (`properties` and `required` beside `object`, `items` beside `array`, `enum` beside `string`), is refused where it is written, and so is a value of the wrong form under a word: `properties` that is not a map, `required` or `enum` that is not a list, `nullable` or `additionalProperties` that is not true or false. A required name the shape does not list under `properties` is refused at the name. An integer is a number with no fraction and no exponent, not `-0`, within 64 bits, whichever reader made the value: a reader that keeps a number's text and `serde_json` agree on each. A caller words a `Misfit` in its own terms. A pack may take fewer of the seven words than the module does, and refuses the others under its own code; it takes no other. The shape is written into a pack's payload by `serde`, so a change to `Shape`'s fields changes compiled targets ([decision 0404](../../decisions/0404-one-closed-shape-of-a-body-in-the-provider-contract-and-the-stricter-refusal-where-the-two-copies-differed.md)).

### A pack, whole

```rust
pub struct ExamplePack;

impl Pack for ExamplePack {
    fn manifest(&self) -> &'static str {
        include_str!("../anything2mcp-provider.toml")       // declares the mode `example`, its steps, the file kind, the discoverer
    }

    fn register(&self, cx: &PackContext, to: &mut Registrar<'_>) -> Result<(), Failure> {
        let mode = Mode::new("example")?;
        to.driver(mode, Arc::new(ExampleDriver { cx: cx.clone() }))?;
        to.compiler(
            CompilerRegistrations::new()
                .surface_kind(example_surface_kind())           // a SurfaceKindSpec
                .file_kind(Arc::new(ExampleFiles::new()))       // implements `FileKind` of the files crate
                .lowering(Arc::new(ExampleLowering::new())),    // implements `Lowering` of the files crate
        )?;
        to.discoverer(Arc::new(ExampleDiscoverer), Arc::new(ExampleDescriber))
    }
}

struct ExampleDriver { cx: PackContext }

impl Driver for ExampleDriver {
    fn prepare<'a>(&'a self, cx: &'a PrepareContext<'a>, _lease: Option<&'a SessionLease>)
        -> BoxFuture<'a, Result<Prepared, StepFailure>> {
        Box::pin(async move { Ok(Prepared::new(ExampleClient::open(&cx.surface.surface.address))) })
    }
    fn act<'a>(&'a self, prepared: &'a mut Prepared, step: &'a Step, input: &'a StepInput<'a>)
        -> BoxFuture<'a, Result<StepOutcome, StepFailure>> {
        Box::pin(async move {
            let client = prepared.state::<ExampleClient>().ok_or_else(|| StepFailure::not_sent(not_prepared()))?;
            let values = client.carry_out(step, input.args).await?;   // its error type converts to StepFailure with `sent` set
            Ok(StepOutcome::Done(Observed { values, ..Observed::default() }))
        })
    }
    fn health(&self, _surface: &SurfaceRef) -> DriverHealth { DriverHealth::Ready }
    fn release<'a>(&'a self, _prepared: Prepared, _poisoned: bool) -> BoxFuture<'a, Cleanup> {
        Box::pin(async move { Cleanup::complete() })
    }
}
```

## The supervisor

Built: one-shot runs, foreign links ([decision 0090](../../decisions/0090-the-supervisor-ends-a-group-only-while-its-leader-is-unreaped.md)), and links over a program's standard streams on every system, and the check of who may change a program that every pack starting one calls ([decision 0250](../../decisions/0250-one-check-of-who-may-change-a-program-and-the-mcp-packs-local-server-on-windows.md)). Not built: the framed link with its handshake, the queue, and ownership records. The table of rules below is the design; the rows on the handshake, the framing, the queue, and proving ownership by the owner mark describe what is not built yet.

```rust
#[derive(Clone)]
pub struct Supervisor { /* limits, running subsystems, leftovers */ }
impl Supervisor {
    pub fn new(limits: SupervisorLimits) -> Self;              // per_kind: 16 unless a kind is listed
    /// A program run to completion.
    pub async fn run(&self, spec: LaunchSpec, link: OneShot) -> Result<Finished, ConnFailure>;
    /// A program that speaks its own protocol over a pair of pipes, until it is stopped.
    pub async fn start_foreign(&self, spec: LaunchSpec, pipe: ForeignPipe) -> Result<ForeignLink, Failure>;
    /// The same over the program's standard input and output, on every system (decision 0250): descriptors 0 and 1
    /// on Linux and macOS; its standard handles on Windows, started with no console window. Standard error: the null device.
    pub async fn start_on_standard_streams(&self, spec: LaunchSpec) -> Result<ForeignLink, Failure>;
    pub fn list(&self) -> Vec<SubsystemListing>;               // identifier, kind, place, for whom, link, process, state
    pub async fn stop(&self, id: SubsystemId) -> Cleanup;
    pub async fn shutdown(&self) -> Cleanup;                   // every caller that asks joins the same cleanup; nothing starts afterwards
    pub fn leftovers(&self) -> Vec<Leftover>;   pub fn clear_leftovers(&self);
}
pub struct LaunchSpec {
    pub program: ProgramRef,                                   // an absolute path, and the digest declared for it, if any
    pub args: Vec<OsString>,                                   // a list, never a shell line
    pub env: EnvAllowlist,                                     // the child's whole environment
    pub cwd: Option<CheckedPath>,                              // None: a private folder made for the start, removed after (decision 0271)
    pub kind: SubsystemKind,                                   // counted by kind
    pub place: Option<String>,                                 // one subsystem of a kind holds a place
}
pub struct ProgramRef { pub path: PathBuf, pub digest: Option<Digest> }
// impl From<&DeclaredProgram> for ProgramRef, from the compiled target type of the core crate: the engine passes `surface.programs.iter().map(ProgramRef::from).collect()`
// to `DriverHandle::supervisor`, where `surface` is the `CompiledSurface` of the run (decision 0124).
#[derive(Default)]
pub struct EnvAllowlist { pub pass: Vec<String>, pub set: Vec<(String, OsString)> }   // names passed through; pairs set. Never a secret
pub struct OneShot { pub max_output_bytes: u64, pub time_limit: Duration, pub stdin: Option<Vec<u8>> }
pub struct ForeignPipe {
    pub child_reads_fd: i32, pub child_writes_fd: i32,         // Linux and macOS: the child's two descriptors, 3 and 4 for a browser
    pub windows_argument: String,                              // Windows: one argument added last; `{read}` and `{write}` become the handle values
}
pub enum LinkKind { OneShot(OneShot), Foreign(ForeignPipe), Standard }
pub struct Finished {
    pub exit: Exit,                                            // Exit::Code(i32) | Exit::Killed: ended from outside (on Windows: by the supervisor)
    pub stdout: Vec<u8>, pub stderr: Vec<u8>,                  // each up to max_output_bytes
    pub stdout_bytes: u64, pub stderr_bytes: u64,              // all that was written; truncated() says whether more than was kept
    pub program_digest: Digest, pub verified: bool,            // verified: the digest is the declared one
    pub cleanup: Cleanup,
}
pub struct ForeignLink {
    pub id: SubsystemId,
    pub to_child: Box<dyn AsyncWrite + Send + Unpin>, pub from_child: Box<dyn AsyncRead + Send + Unpin>,
    pub program_digest: Digest, pub verified: bool,
}
/// What a pack gets. Its allowlist is the manifest's `processes` and the paths of its requirements, plus,
/// where `compiled_target_programs` is true, the programs the engine names from the published compiled target. Anything else is `effect_not_declared`.
/// Each carries the digests declared for it: a process's `digest`, a requirement's `known_digests`, a compiled target program's digest.
#[derive(Clone)]
pub struct ScopedSupervisor { /* the supervisor, the provider, the declared programs */ }
impl ScopedSupervisor {
    pub async fn run(&self, spec: LaunchSpec, link: OneShot) -> Result<Finished, ConnFailure>;
    pub async fn start_foreign(&self, spec: LaunchSpec, pipe: ForeignPipe) -> Result<ForeignLink, Failure>;
    pub async fn start_on_standard_streams(&self, spec: LaunchSpec) -> Result<ForeignLink, Failure>;
    pub async fn stop(&self, id: SubsystemId) -> Cleanup;      // only what this provider started
}
pub struct ConnFailure { pub failure: Failure, pub ran: Ran }
pub enum Ran { NotStarted, Unknown }
pub struct Cleanup { pub complete: bool, pub remaining: Vec<Leftover> }   // Cleanup::complete(), Cleanup::incomplete(remaining)
pub struct Leftover { pub kind: Option<SubsystemKind>, pub what: String }

// Who may change a program (decision 0250, moved here from the command pack, where decision 0216 built it).
// Blocking: a pack calls it off the async threads, before it asks the supervisor to start the program.
pub fn check_program(path: &Path, trust: &ProgramTrust, role: ProgramRole) -> Result<(), ProgramRefused>;
pub enum ProgramRole { Program, Interpreter, OwnCode, Script, Folder }   // OwnCode: a program of the closed list below; Script: a file the engine wrote; Folder: one it made
pub struct ProgramTrust { /* group ids */ }                     // ProgramTrust::with_groups(ids); default: the system's own only
pub enum ProgramRefused { NotAbsolute, Missing, Script, Writable(PathBuf), Unreadable(PathBuf), Link(PathBuf), Elsewhere }
pub fn is_shell_or_interpreter(path: &str) -> bool;             // the name list
pub fn reads_its_line_as_code(path: &str) -> bool;              // cmd, command, wscript, cscript, mshta, a batch file
pub fn interpreter_form(path: &str, fixed: &[String]) -> Result<(), InterpreterRefused>;   // the closed table
pub enum InterpreterRefused { ReadsItsLineAsCode, NotKnown, NotItsForm(&'static [&'static str]) }
pub fn runs_its_own_code(path: &str) -> Option<&'static OwnCode>;   // the closed list of commands that run no code but the program's own (decision 0349)
pub struct OwnCode { pub kind: OwnCodeKind, pub names: &'static [&'static str], pub forms: &'static [OwnCodeForm] }
pub enum OwnCodeKind { Launcher, VersionManager, ContainerRunner }
pub struct OwnCodeForm { pub args: &'static [&'static str], pub names: bool }   // names: `--` and values may follow
pub fn redirects_loading(name: &str) -> bool;                  // a variable that makes a program load other code
```

What the built part holds:

| Rule | How it is held |
| --- | --- |
| Identify at every start | The program is named by an absolute path. At every start, immediately before it, the supervisor resolves the path once, opens the file once without following a link at its name, takes only a regular file, and digests it through that handle. Every digest declared for the program must be the one found: the caller's, a manifest process's, a requirement's `known_digests`, a published compiled target's. One that differs is refused, `policy`, `effect_not_declared`, and nothing starts. With no declared digest it starts and is marked unverified. No digest is remembered between starts |
| Start what was digested | On Linux the program is started from the handle it was digested through, by the name the system gives an open file, so nothing done to its path meanwhile changes what runs; the program still sees the path it was asked for by as its own name. That cannot be done for a file that starts with `#!`, which the system hands to an interpreter by name, nor without the process file system, and macOS cannot start an open file at all. There the program is started by its path after a last look, under the start lock, that the path leads to the file that was digested, with the same device, file number, size, and time of last change; a replacement between that look and the start is not seen, and is `effect_not_declared` when it is. On Windows the file is held open, shared for reading only, from before the digest until the process exists: nobody can write, rename, or delete it meanwhile, and a file someone holds open for writing is not started. A folder on the way is not held there; the path is resolved once more under the start lock and must lead to the same file |
| Off the async threads | Resolving, reading, and starting are one piece of blocking work. If the caller's future is dropped meanwhile, what was started is dropped with its result, which ends its group |
| Nothing inherited | The child's environment is cleared; only the names listed are passed through and only the pairs listed are set, plus the owner mark, 128 random bits in `ANYTHING2MCP_OWNER_MARK`. A variable whose name starts with `A2M_`, in any letter case, is the engine's own: it is never copied from the engine's environment into a child, and a launch that names one is refused before the start, `policy`, `engine_variable_refused` ([decision 0158](../../decisions/0158-the-product-crate-and-the-local-engine-after-their-safety-audit.md), finding P6). `is_engine_variable` is the one test of the name. A variable that makes a program load code other than its own, or run a program it names (`redirects_loading`: the `LD_` and `DYLD_` variables, the shells' start-up files and imported functions, the start-up and module-path variables of the language runtimes, the .NET Framework's and .NET's settings, OpenSSL's configuration, `COMSPEC` and `SHELL`, and helper commands), is refused the same way, `policy`, `variable_refused`, and never copied or set ([decisions 0250](../../decisions/0250-one-check-of-who-may-change-a-program-and-the-mcp-packs-local-server-on-windows.md) and [0271](../../decisions/0271-the-check-of-programs-the-product-starts-after-a-second-reader.md)). A `PATH` the launch passes or sets is held, folder by folder, to `check_program` as a `Folder`, or for a folder that is not there its nearest folder that is; a folder another user could change, or a name that is not absolute, is `policy`, `variable_refused` (decision 0271) |
| Its own working folder | A launch with no `cwd` runs in a folder made for that start: in the system's temporary folder, under a random name, mode `0700` or the private access list, held to `check_program` as a `Folder` before the start, and removed when the run is reaped or the link is stopped (decision 0271) |
| Who may change it | Not the supervisor's own check: every pack that starts a program calls `check_program` before it asks for the start. On Unix the path is walked name by name from the root, links followed, each folder and link required to be the engine's user's or the administrator's and writable by nobody else, a group's write counting only for the groups `ProgramTrust` holds (on macOS the group 80 always). On Windows the owner and the access list of the drive's root, each folder, and the file are read, by the rights of [decision 0216](../../decisions/0216-the-command-pack-on-three-systems-the-administrators-group-and-a-script-the-target-keeps.md); a reparse point, a share, and an unreadable list are refusals. On macOS any entry of a file's or a folder's extended access list that allows is a refusal; on Linux a POSIX access list is a refusal where a group's write was accepted because `ProgramTrust` holds the group. On Windows the drive must be a fixed local disk whose letter stands for a whole volume: a mapped share or a `subst` letter is `Elsewhere` (decision 0271). `Program` refuses shells, interpreters, launchers, package runners, container runners, build tools, programs that start another, and batch files by name, and every file that does not begin as this system's programs do (an executable and linkable file, Mach-O, or the DOS header); `Interpreter` refuses only `reads_its_line_as_code` by name, and the same first bytes; `OwnCode` refuses by name every program, by its declared name or the name of the file its path leads to, that has no row of `runs_its_own_code`, and the same first bytes (decision 0349). `CheckedPath::create_private_dir` and `create_private_dir_all` make a folder the engine's user's alone (mode `0700`; a protected access list naming its owner, the local system account, and the Administrators group on Windows) |
| A group the supervisor can end | A process group on Linux and macOS, signalled only while its leader is unreaped; a job object on Windows that ends its processes when its last handle closes. On Windows the child is created suspended, placed in the job, and only then resumed, so it runs nothing outside the job and whatever it starts at once is in the job too; its first thread is found in a snapshot of the system's threads, because the standard library does not hand out its handle. On macOS the answer "not permitted" from a group whose leader has exited means nothing is left in it. macOS gives the same answer while the leader is in the middle of its exit, its files closed and its exit not yet seen by a wait; so that answer, with the leader not yet seen to exit, is judged again once the leader has exited, after at most ten seconds, and a leader that does not exit by then is a group that cannot be ended |
| One start at a time | Every start holds one lock from before its pipes are made until the child exists, on every operating system, so that no child inherits another's pipe or handle |
| Bounded | A one-shot run has a time limit, at which the whole group is ended and the run is `transient`, `timeout` with `Ran::Unknown`; each output stream is kept up to its bound, and read and counted beyond it. Once the group is gone the run takes what is in its pipes and does not wait for their end, which a process outside the group may hold back; such a holder is no part of the cleanup verdict |
| An ended program is told from an exited one | On Linux and macOS a program ended by a signal has no exit status, and is `Exit::Killed`. Windows has no signals, so the supervisor ends a job with an exit status of its own, and a program that shows it is `Exit::Killed`; a program someone else ended there shows the status that ender named |
| Nothing left behind | At a program's exit its whole group is ended; `stop` and `shutdown` end the group and wait for it; a run whose future is dropped ends its group |
| Counted, and one per place | Each kind has a limit, and a taken place is refused, both `busy`, `subsystem_limit` |
| Incomplete cleanup is a state | It names what remains and blocks new starts of that kind with `busy`, `cleanup_incomplete`, until a person clears it |

The design in full:

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

Two cases section 204 does not cover:

| Link | Identified and owned by | What the supervisor still does |
| --- | --- | --- |
| `Foreign`: a protocol someone else fixed | The digest, the version the program reports through its own protocol, and the owner mark. It cannot return the launch identifier | Starts, counts, bounds, and stops it; the pack frames the bytes, and a frame the pack cannot parse still closes the link |
| `OneShot`: a program run to completion, such as a command | The digest and the owner mark; there is no handshake | Arguments as a list, never a shell line; the environment allowlist; a checked working directory; output cut at its bound and counted; the deadline; the whole group ended at exit or deadline. A deadline passed on a committing run is `Ran::Unknown` |

## The plug-in host and protocol

The host reads a plug-in's manifest at build time and registers proxies: `PluginDriver`, `PluginDiscoverer`, `PluginDescriber`, `PluginAuthProvider`, `PluginSourceReader`, and compiler registrations whose lowerings and lint rules are calls. The process is started under the supervisor on first use, never at registration. Schemas cross the boundary as the files the manifest names; lowerings and lint rules cross as calls made at compile time, and the compiled target records the plug-in's digest, so a changed plug-in is a changed compiled target.

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
| The product crate | `RegistryBuilder`, `HostServices`, `ProviderSettings`, `Pack`, `Supervisor::new`, `Roots::add` |
| `a2m-engine` | `Registry::driver`, `DriverHandle`, `archetype`, `mode_defaults`, `source_reader`; implements `SandboxPort`, or uses `ReadOnlyPort` |
| `a2m-actions` | `listing`, `discoverers_for` when it submits a discovery job, `auth_providers`, `compiler_set`, `action` for its dispatcher, `Roots::check` for the kernel's own reads, `supervisor` for `doctor.run` |
| A runner | `discoverers_for` and `Discoverer::discover`, inside the engine's sandbox, for a discovery job |
| Packs | `Pack`, `PackContext`, `Registrar`, `CompilerRegistrations`, the traits, `PrepareContext`, `CheckedPath`, `ScopedSupervisor` to start their programs; `shape`, in the HTTP pack and the web pack |
| `a2m-sched` | Nothing. Mode defaults reach it as data in a job's requirements |

It calls: `a2m-files` to accept compiler registrations; the pack objects it holds. It emits no spans of its own yet.

## State

Memory only: the registry snapshot; for each running subsystem its kind, place, and group; the leftovers of stops that were incomplete. The supervisor's ownership records will be written through `a2m-ledger` from build step 7, so that a crashed engine's leftovers can be recognized at the next start; they are not built. Settings choose which providers are enabled; they are read by the product crate, not here.

## Failures

The codes are declared in this crate's `failure_codes!` table, the module `codes`, each with its meaning, its next step, and, for a `busy` code, whether it passes by itself ([decision 0258](../../decisions/0258-every-crate-declares-its-failure-codes-in-one-table.md)); `codes::ALL` lists each with its one class.

| Class | Code | When |
| --- | --- | --- |
| `input` | `manifest_invalid`, `contract_unsupported`, `mode_conflict`, `provider_duplicate` | A manifest fails its checks |
| `input` | `path_not_allowed`, `file_unavailable` | `Roots::check` refuses a path, a manifest names a program by a path that is not allowed, or a write or a removal meets a link; a checked file cannot be read or written, or a folder to be made new is already there |
| `input` | `provider_ambiguous`, `mode_unknown`, `not_registered` | Two ready providers and no setting; a mode nobody drives; a reader nobody registered |
| `input` | `kind_owner_missing`, `kind_owner_conflict` | A kind a provider reads has no enabled owner; two enabled packs own one kind for one surface kind |
| `input` | `pack_settings_invalid`, `program_not_found` | A pack's settings do not fit its schema; a program is not at its declared path |
| `policy` | `effect_not_declared`, `engine_variable_refused`, `signature_invalid`, `intent_missing`, `read_only_posture` | A registration, a start, or a root beyond the manifest, a program whose digest is not a declared one, or one replaced between its check and its start; a bad signature; a `commits` step without its token, or a step of a kind that always commits sent without the mark; a `commits` step where nothing may be written |
| `busy` | `provider_unavailable`, `subsystem_limit`, `cleanup_incomplete`, `shutting_down`, `start_failed`, `queue_full` | Not ready, with the availability state; the kind's limit or a taken place; leftovers block a start; the supervisor is stopping; the operating system did not start the program; a full subsystem queue |
| `unknown_variant` | `undeclared_code`, `unexpected_absent`, `step_kind_unknown`, `reconcile_too_late`, `finding_not_declared`, `handshake_failed` | A driver invents a code; `Absent` from a step that is no lookup; a step of a kind the mode did not declare; `for_reconcile` from a committing step; a finding of an undeclared kind or level; a subsystem fails its handshake |
| `transient` | `connection_lost`, `timeout` | A stream broke or a time limit passed on something that commits nothing |
| `possible_commit` | `no_confirmation` | A `commits` step whose fate is unknown, or that was sent and failed as `policy` or `input` |

`signature_invalid`, `queue_full`, `handshake_failed`, and `connection_lost` are reserved for the parts not built.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| A manifest that asks for an undeclared effect, or registers beyond it, is refused with a named failure | `tests/registry.rs`: an undeclared point, an undeclared session, and a pack that registers a driver its manifest does not list |
| A mode registered twice, or with two archetypes, is refused | `tests/registry.rs`, in one manifest and across two packs in both orders |
| Registration order never changes the registry's digest or any choice; a changed registration changes the digest | `tests/registry.rs` builds three packs in all six orders, then with a pack, an object, and a setting changed |
| Two ready providers and no setting give `provider_ambiguous`, never a choice; a provider that is not ready is never replaced | `tests/registry.rs` |
| A kind has one owner for a surface kind, and a reader of a missing owner stays registered | `tests/registry.rs` |
| A `commits` step without the token of its run, lease number, and record never reaches a driver | `tests/handle.rs`, with real tokens from a real store and a stub driver that records what reaches it |
| A doubtful write is always `possible_commit` | `tests/handle.rs`: a stub driver returning `transient` with `Sent::Unknown` |
| A write that was sent is never settled by a class that means unsent | `tests/handle.rs`: `policy` and `input` answered on a committing step become `possible_commit`; unsent, or on a step that commits nothing, they stand; `rejected` and `stale` stand |
| A step of a kind that always commits is never carried out as a read | `tests/handle.rs`: through the handle and through `ReadOnlyPort`, with a mode that lists the kind |
| A driver cannot return, and a manifest cannot declare, a code that is the kernel's alone | `tests/handle.rs`, also through a step's `fails_as`; `tests/registry.rs`, every code of the list |
| `for_reconcile` on a committing step, `Absent` from a step that is no lookup, and an undeclared code are refused | `tests/handle.rs` |
| A discoverer cannot write | By the types, as stated under the contracts above; and `tests/handle.rs` sends a committing step through `ReadOnlyPort` and sees it refused before the driver |
| An availability check starts no process | By construction: the check has no way to reach the supervisor. No test counts processes |
| A path outside its root, through `..`, an absolute or drive or network form, or a link, can never become a `CheckedPath` | `tests/paths.rs`: thirty-seven escape forms, refused on every operating system; planted links, on Linux and macOS, and on Windows where the test may make a link |
| A write never goes through a link, at its temporary file's name or at its own | `tests/paths.rs`: a link planted at the old predictable name `<name>.a2m-new` leads outside the root, and the file outside is asked what it holds; a link at the final name, there before the check or put there after. A unit test of `path.rs` chooses the temporary name itself and plants a file, a link to a file outside, and a link to nothing at it. All of them run on Linux and macOS, and on Windows where the test may make a link, which the CI runners may; where it may not, the test ends early and proves nothing there |
| A file swapped for a link after the check is not opened, and a folder on the way swapped for a link is not passed | `tests/paths.rs`, on Linux and macOS only |
| A folder made new is made only where nothing was | `tests/paths.rs` |
| A pack can start only what its manifest and the published compiled target declare | `tests/supervisor.rs`: a pack whose manifest names the stand's program starts it, and is refused another |
| A program's declared digests are checked at every start, on the file that is started | `tests/supervisor.rs`: a required program with a known digest is replaced after the registry was built, by a new file and in place with the same length, and is not started, which the stand is asked; a declared link repointed between two starts; a script. On Linux a test shows that the process was started from the handle and not by the path |
| On Windows a child runs nothing before it is in its job | Unit tests of `supervisor/windows.rs`, which only a Windows machine runs: a child created suspended does not exit until it is resumed; a child that starts a second process at once has both in its job, and ending the job leaves neither; a held program cannot be written, renamed, or deleted. They have been compiled and linted for Windows and have not been run |
| A one-shot run captures its output, is ended at its time limit with everything it started, keeps output to its bound, and leaves no process | `tests/supervisor.rs`, against the command-line test stand, which is asked which of its children are alive |
| A process outside the group that holds a run's output neither delays the run nor spoils its cleanup verdict | `tests/supervisor.rs`, on Linux, with a sleeper that left the group and kept the pipe. The tests start programs only through the supervisor, so its start lock covers them; the stand builds its binaries before any test starts one |
| A foreign link carries bytes both ways on descriptors 3 and 4, and stopping it ends the group | `tests/supervisor.rs`, on Linux and macOS. On Windows a unit test of the supervisor carries bytes both ways over the two named pipes, with a thread using the child's ends as a child does; the engine's ends are connected when they are made, without which they carry nothing. Handing the two handles to a real child there is proven by the browser engine's tests |
| A malformed or oversized frame closes the connection and fails every pending request | Not built |
| The supervisor never signals a process it cannot prove it started | By construction on Linux and macOS: a group is signalled only while its leader is unreaped (decision 0090). No test places a foreign process in reach |
| No secret appears in a value, a capture, or a failure that leaves the handle | `tests/handle.rs`: a stub driver that returns a registered secret in a value, in a list, in a value for reconcile, in a capture's name, in a capture of text, and in a failure. A capture that is not text is not searched. The frame log is not built |

## Build order

1. Done: manifest parsing and checks, `RegistryBuilder`, `Pack`, `PackContext`, pack settings. Package B3.
2. Done: `Driver`, `DriverHandle`, `Authority`, and the choice rule, with a stub driver. Slice 1.1's contract half; the first real driver is a pack's.
3. Done: `CompilerRegistrations` gathered into `CompilerSet`. Slice 1.3's contract half.
4. Done: the `commits` check against a real `IntentToken`. Slice 3.3's contract half.
5. Done: `Discoverer`, `Describer`, `SandboxPort`, `ReadOnlyPort`, `ProviderAction`. Package E1's contract half.
6. Done: the supervisor for `OneShot` and `Foreign` links, `ScopedSupervisor`, and `CheckedPath`. Done after the safety audit (decision 0148): digests at every start on one opened file, the suspended start on Windows, writes that never go through a link, `entry_kind`, `create_dir`, the handle's rule for a write that was sent, its backstop redaction, `commit_steps`, and the kernel's own codes. Not built: `Framed` links with the handshake, the request queue, and joining a start for a place that is taken.
7. Not built: ownership records through `a2m-ledger`, and leftovers recognized at start.
13. Done ([decision 0250](../../decisions/0250-one-check-of-who-may-change-a-program-and-the-mcp-packs-local-server-on-windows.md)): `check_program` and its tests, moved from the command pack, with the roles `Script` and `Folder`; the closed table `interpreter_form` and `reads_its_line_as_code`; the closed list `redirects_loading`, refused by the supervisor as `policy`/`variable_refused`; `LinkKind::Standard` and `start_on_standard_streams`, which on Windows gives the child the two pipes as its standard handles and `CREATE_NO_WINDOW`; `CheckedPath::create_private_dir` and `create_private_dir_all`. The Windows half compiled and linted from Linux; CI is its first run.
14. Done ([decision 0271](../../decisions/0271-the-check-of-programs-the-product-starts-after-a-second-reader.md)), after the sixth safety audit's section D: the loading-variable list extended by family; a `PATH` held folder by folder; launchers by kind in the name list, and a version after a name; macOS access lists and Linux access lists behind a trusted group; a fixed local disk and a whole-volume letter on Windows; a private working folder per start; a program's first bytes. Not done: a fresh walk before each start in the command pack (owed there), and an explicit handle list for a foreign link's ends on Windows (the standard library offers none on stable Rust). The macOS and Windows halves compiled and linted from Linux; CI is their first run.
14a. Done ([decision 0349](../../decisions/0349-the-two-steps-of-decision-0299-as-built-a-closed-list-of-commands-and-the-javascript-runtime-as-a-launcher.md), packages L1 and L2): the closed list `runs_its_own_code` of the commands of launchers, version managers, and container runners that run no code but the program's own, each traced on a machine that had the program; the role `ProgramRole::OwnCode`; and the JavaScript runtime as a row of the table of interpreter forms. Tests: `the_programs_whose_listed_commands_run_their_own_code_are_known_by_name` and `a_program_of_the_closed_list_passes_the_check_only_in_its_own_role` in `src/supervisor/program/tests.rs`. Not built: what the MCP pack needs to start a server's installed entry file through a launcher (package L3).
8. Not built: the plug-in host and protocol version 1, and the check of a plug-in's signature. A plug-in found on the machine is listed as present and not enabled. Package F3 for listing; Phase 8 for driving.
11. Done ([decision 0193](../../decisions/0193-the-core-the-contract-the-compiler-and-the-scheduler-after-the-second-pass.md)): `Temporary::replace` puts in place only the file it made, by identity before the rename and after it; on Windows the identity is the 128-bit `FileIdInfo` with the 64-bit index where a file system gives none, the temporary file is shared with nobody while it is written, and `keep_owner_and_mode` answers `true` only where the target has the very owner and access list the new file took, so a target with an access list of its own is told apart (compiled and linted from a copy; CI's Windows job runs `nobody_else_opens_the_temporary_file_while_it_is_written`, `a_file_or_link_swapped_in_at_the_temporary_name_is_never_put_in_place`, and `a_target_with_an_access_list_of_its_own_is_told_apart_on_windows`); a refused manifest that says a name another provider holds is unnamed; the recorded-elsewhere question; roots of each request's project.
10. Done ([decision 0176](../../decisions/0176-the-action-registry-takes-up-the-kernels-second-pass.md)): a save's temporary file is made private (mode 0600 on Unix), `Temporary::keep_owner_and_mode`, `Temporary::identity`, `FileIdentity::of_open`, and a temporary file removed only while its name still holds it; `RefusedProvider` and `Registry::refused`, so a provider whose manifest was refused is named with its problem. On Windows the temporary file takes the access list its folder passes down, and `keep_owner_and_mode` keeps the read-only flag alone and answers `false`.
11. Done ([decision 0177](../../decisions/0177-a-targets-offline-tests-run-against-a-replay-of-their-fixtures.md)): `Replay`, `RecordedAnswer`, `Recorded`, `answers_to`, `answer_name` (the last part of a step's entry, the name a fixture's file carries, since [decision 0365](../../decisions/0365-an-mcp-servers-json-answered-as-text-is-a-declared-result-form-and-the-pack-speaks-the-earlier-handshake-revision.md)), and `Registry::replay_driver`, the replay every pack gets from the contract; the contract's code `unknown_variant`/`fixture_no_answer`. Then ([decision 0180](../../decisions/0180-a-targets-offline-tests-cover-writes-and-gate-the-publish.md)): `RecordedAnswer::repeats`, an answer that answers every step of its name, and `Replay::sent`, what the engine sent to each committing step.
9. Done (decision 0162): what earlier steps recorded (`Prepared::recorded`, `DriverHandle::act_recorded`); how a step was done (`Capture::acting`, `Observed::acting`); what the system said (`StepFailure::with_said`, kept by the handle through its redaction and through a write in doubt); the project folder (`CheckedPaths::with_project`, `project`, `project_folder`, `input`/`project_folder_missing`); the save in steps, a file's identity, and `may_write`; `StepInput::new`. Tests: `tests/handle.rs`, `tests/paths.rs`.
15. Done ([decision 0263](../../decisions/0263-a-person-hands-the-scout-files-of-the-projects-sources-folder-by-names-the-discoverers-declare.md)): `DiscovererDecl::files`, the names under which a discoverer reads files a person hands a discovery, read from the manifest's `files` (none when left out) and checked as names; `Registry::files_taken`. A contract change: a manifest that lists `files` is refused by a build before it. `tests/registry.rs`: `a_manifest_without_files_declares_none_and_one_with_files_is_read`.
12. Done ([decision 0240](../../decisions/0240-one-typed-reading-of-where-an-address-is-and-one-question-of-a-plain-link.md)): each destination carries its scope, read by `ScopedHost::of_destination` from its own address when `Destinations::new` builds the set, and returned by `scope_of`. Nothing else sets it, and the allow-list still compares addresses as written. `tests/destinations.rs`; the engine's `a_destinations_scope_comes_from_its_address_and_never_from_an_input`. Not built: any check that decides by a scope.
13. Done ([decision 0313](../../decisions/0313-the-place-of-an-address-is-derived-from-its-scope-and-the-machines-subnets.md)): `NetworkPlace`, `Placed`, `MachineState`, and `Destinations::place_of`: the place of an address, derived from its scope and the subnets of the machine's interfaces that are up, never from an input and never by resolving a name. `src/place.rs` unit tests; `tests/destinations.rs`. Not built: any check that decides from a place, and `via_gateway`.
14. Done in part ([decision 0327](../../decisions/0327-a-reach-is-a-record-in-force-only-on-the-network-it-was-named-on-and-via-gateway-is-derived-from-it.md), slice N3): `Reach`, `ReachEntry`, `GatewayName`, `PlacedInReach`, and `NetworkPlace::of_in_reach`, which gives `via_gateway` only where a reach in force names a gateway for the destination, and so the place of a destination now depends on the network the machine is on. A reach is in force only where its fingerprint (`a2m-netstate`) is recognized as the current one. Tests: `reach.rs`. Not built: the protected store of reaches and the person's actions to name, list, and withdraw one, which are `a2m-actions`'.
17. Done ([decision 0391](../../decisions/0391-traffic-capture-is-a-capability-of-the-provider-contract-that-yields-structure-only.md), package E2): `points.traffic_capture`, `CaptureSource`, `TrafficCapture`, `CapturedTraffic`, `CapturedRequest`, `take_capture`, `kept_by_structure`, `Registrar::traffic_capture`, `Registry::captures_for`, `CaptureHandle`, `Capturing`. Tests: `tests/capture.rs`. No action handler opens a capture through the registry yet.
16. Done ([decision 0343](../../decisions/0343-the-decompiling-engine-is-a-discoverer-of-the-provider-contract-that-starts-only-a-pinned-program-and-reads-one-form.md), package X6): the module `decompiler`: `Decompiler` (the place the person named and the digests the product pins; `identify` looks at the program and starts nothing; `decompile` is handed a `CheckedPath` of a regular file and never a process), `DecompilerSpec`, `DecompilerDiscoverer`, and `DecompilerDescriber`; the codes `decompiler_not_installed`, `decompiler_not_pinned`, `decompiler_spec_invalid`, `decompiler_failed`, and `decompiler_output_unknown`; the stand `targets/example-decompiler`. The findings are `artifact`, `inferred`, and carry the engine's digest. Not built: the settings and the pack that register it, and the first engine's adapter and pinned digests.
18. Done ([decision 0404](../../decisions/0404-one-closed-shape-of-a-body-in-the-provider-contract-and-the-stricter-refusal-where-the-two-copies-differed.md), package E7a): the module `shape`, moved from the HTTP pack, which used it alone, and from the web pack's private copy; both packs read and check their shapes through it. Where the two copies refused different things, the stricter refusal holds in both, each with a test in `src/shape.rs`. Not built: its use by the browser base (package E7b).

## Open points

- Whether an upstream MCP server's mapping file is its manifest. This design assumes so, listed under the `mcp` pack, and checked by that pack's file kind.
- Which operating-system mechanism backs `confined` on each platform. Until it is researched, a plug-in's effects are listed as `declared`.
- The widget driver extension point has no trait yet. First-party widget drivers stay inside the web pack's driver; the trait is written when a plug-in first needs to supply one.
- A process that leaves its group on Linux or macOS is not found, even when it holds the program's output, and nothing ends the children of an engine that crashed there; both wait for the ownership records. The macOS answer for a group with only its exited leader left is taken from the system's documented behavior and has not been run from here. Whether job-object membership is enough on Windows in every case.
- Where the published compiled target names the programs a command surface starts, so that the engine can pass them to `DriverHandle::supervisor` without reading a pack's payload.
- A requirement's version is not checked, because the availability check starts nothing.
- Whether `Posture::Discovery` and `Posture::Sandbox` should be one posture with a flag; they differ today only in whether test records exist.
- The Windows code of the supervisor has been compiled and linted, and has not been run. The next Windows run must show the three unit tests of `supervisor/windows.rs` named under the invariants passing, and `tests/supervisor.rs` unchanged: a time limit ends a program and the child it started, and no process is left. If a child is never resumed, every start there fails with `busy`, `start_failed`, or runs into its time limit.
- An attacker who can write a program's file in place, and not only replace it at its path, can change it between its digest and its start on Linux and macOS, where nothing denies a write. The digest is therefore no defence against the owner of the program file. A digest remembered by the file's identity was weighed and not built (decision 0148): the times of a file can be set back by whoever may write it, on Windows all of them.
- `CheckedPath` on Windows, and `exists`, `is_dir`, `size`, `list`, and `create_dir_all` everywhere, check and then use the path by name.
- Done (decision 0162): a save in steps. `CheckedPath::create_temporary` gives a `Temporary`, made new under a random name in the target's folder and handed back open; `write`, `flush`, `read_back` for the caller's check, `replace` (only the target's name, never through a link), then `CheckedPath::flush_folder`, each a call of its own; dropped before `replace`, it is removed. `CheckedPath::identity` gives a `FileIdentity` (device and file number; volume and file identifier on Windows, by one unsafe call). `CheckedPath::may_write` opens the file for writing and closes it, writing nothing. Failures tell `busy`/`path_locked`, `busy`/`disk_full`, `input`/`path_denied`, and `input`/`path_missing` apart. The Windows half was compiled and linted from a copy and not run; on Windows `flush_folder` does nothing more, because the file system journals a folder's names.
- The contract's additions of decision 0162 are additions only: `Prepared::recorded` and `DriverHandle::act_recorded`, not a field of `StepInput`; the `acting` capture, not a field of `Observed`; what the system said on `Failure`, not on `StepFailure`; the project folder on `CheckedPaths`, not on `PrepareContext`. Each of those structs is built field by field in the packs, and a field would have broken every one. `StepInput::new` is there so that a field can be added once the packs build their inputs through it.
- No first-party manifest lists `commit_steps`, so the handle checks nothing there yet.
- Done (decision 0185): `absence_steps` in a mode's manifest, checked to be its own step kinds and none that always commits, and `DriverHandle::proves_absence(steps)`, true only for a non-empty list of steps that commit nothing and are all of such kinds. No first-party manifest lists one yet, so a record of identity `none` in doubt that its read-back does not find stays in doubt for a person in every pack today; a pack that can state it for a lookup adds the line to its manifest.
- A mode's `exclusive` is carried by the registry and reported by the engine's `exclusive_needs`, and nothing submits it to the scheduler; see the scheduler's design.
