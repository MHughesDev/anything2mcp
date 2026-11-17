# `a2m-core`

The words every other crate shares: the closed vocabularies, the checked names that arrive with packs, identifiers, values, failures, findings, the secret wrapper, and the compiled plan as data. It holds types and the rules that make a value well formed. It does nothing.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 1, Types |
| May depend on | Nothing internal |
| Depends on | Outside: `serde`. To add, each verified and recorded: an outside crate for SHA-256, for `Digest` and the plan's hash; the plan's binary encoding, chosen in package 1.3 |
| Built in | Stage B, packages B1 and B2. The vocabularies, `Failure`, and `Secret` exist; the rest is to build. The plan types with package 1.1 |
| Carries out | Spec sections [5](../../spec/05-core-concepts.md), [8](../../spec/08-execution-engine.md), [18](../../spec/18-safety-policy-and-approvals.md), [42](../../spec/42-run-ledger-design.md), [46](../../spec/46-target-compiler-and-plan.md) (what the plan contains), [54](../../spec/54-interaction-archetypes.md), [56](../../spec/56-the-provider-contract.md) (availability), [57](../../spec/57-supervising-subsystems.md) (subsystem states), [58](../../spec/58-discovery-and-findings.md), [59](../../spec/59-the-evidence-ladder.md), [61](../../spec/61-effects-hazards-and-consent.md); the [core types](../../reference/technical-reference.md#core-types); [decision 0036](../../decisions/0036-target-files-are-compiled-into-a-hashed-plan-before-anything-runs.md) |

## Owns

- Every word with a fixed set of values, as an enum whose names are the documents' names.
- The shape of every name that arrives from outside: a mode, a surface kind, a step kind, a verb, a file kind, a lint code, a finding kind, a failure code, a target, an operation.
- The value model: what a field's value can be once it has been read from a file, a caller, or a system.
- `Failure`, the one form in which anything in the kernel reports that it could not do what was asked.
- `Finding` and its `Subject`: what discovery returns, as data.
- `Secret`, and the two interfaces the vault implements: `Redact` and `KeyedDigest`.
- The `Plan` and its `Step`, as immutable data: their encoding, and the check of a plan's hash.

It must never: perform input or output; hold a list of modes, packs, or providers; contain logic that differs by kind of system; compile a plan, evaluate an expression, or decode a pack's payload; depend on another crate in this workspace.

## Interface

A sketch. Built items are marked; the rest is settled by the packages named in the build order.

```rust
// Built. Closed vocabularies: each has ALL, NAMES, as_str, Display, FromStr, and serde by name.
pub enum Archetype { Interface, Request, Command, Bridge, Artifact, Stream, Device }
pub enum EffectClass { Read, Write, Irreversible }            // ordered
pub enum Hazard { None, DataElsewhere, SendsOutside, DeviceFunction, Physical }   // ordered
pub enum FailureClass { /* twelve */ }
pub enum EvidenceLevel { /* seven, ordered; default Inferred */ }
pub enum Coverage { Complete, Partial, Unknown }               // default Unknown
pub enum RunState { /* twelve */ }      impl RunState { pub const fn is_final(self) -> bool; }
pub enum RecordState { /* six */ }
pub enum SessionState { /* eight */ }
pub enum BindingStatus { Draft, Shadow, Verified, Retired }

// To build. Closed vocabularies of the provider contract and the supervisor.
pub enum Availability {                                        // section 56; one per availability check
    Ready, NotEnabled, ComponentMissing, ComponentUnverified,
    UnsupportedHost, UnsupportedVersion, Misconfigured, Unknown,   // Unknown is never reported as Ready
}
pub enum SubsystemState { Idle, Busy, Exited, Unreachable }     // section 57; a long-lived connection's health
pub enum SubsystemMove { Retry, Wait, Restart }                 // what to do, given with each state
pub enum DoctorState {                                         // section 57; what `doctor.run` reports per provider and subsystem
    Healthy, ComponentMissing, RuntimeFailure, UnsupportedHost, UnsupportedTarget, ConfigDrift, Unknown,
}

// Built. Checked names whose values arrive with packs and crates.
pub struct Mode(/* [a-z][a-z0-9_]*, at most 32 */);
pub struct FailureCode(/* the same shape, at most 64 */);

// To build. More checked names of the same shape, each registered by a provider or defined by a crate.
pub struct SurfaceKind(/* at most 32: web, http, mcp, desktop, and what a pack adds */);
pub struct StepKind(/* at most 32: locate, set, request; registered with a lowering */);
pub struct Verb(/* at most 32: read, upsert, call; see the open points */);
pub struct FileKindName(/* at most 32: operation, screen, endpoint */);
pub struct FindingKind(/* at most 32: endpoint, screen, command */);
pub struct LintCode(/* owner and rule, each a checked name, joined by one dot; at most 64: kernel.no_fixed_wait */);

// Built.
pub struct Failure { /* class, code, message, next_step */ }
pub struct Secret(/* prints as [redacted]; no Serialize, no Clone */);
impl Secret { pub fn expose(&self) -> &str; }

// To build: identifiers. Each is a checked name or an opaque id, never a bare String.
pub struct TargetName(/* checked name */);
pub struct OperationName(/* {target}_{module}_{object}_{verb} */);
pub struct SurfaceName(/* checked name, unique within a target */);
pub struct Environment(/* checked name: which copy of the system, such as test or live */);
pub struct Identity(/* who the system sees: a person or a dedicated account; never a credential */);
pub struct RunId(/* opaque; made by the store */);
pub struct RecordKey(/* the parts that find one record again, in declared order */);
pub struct Digest(/* sha-256, as lowercase hex */);

// To build: values. What a field holds after it is read and before it is compared.
pub enum Value {
    Null,
    Bool(bool),
    Text(String),
    Decimal(Decimal),        // exact; never a float
    Instant(Instant),        // a moment, with its zone rule applied
    Date(Date),
    List(Vec<Value>),
    Record(Values),
}
pub struct Values(/* field name to Value, in declared order */);

// To build: redaction and keyed digests. The vault implements both; everything that emits text takes a Redact.
pub trait Redact: Send + Sync {
    /// Returns `text` with every registered secret replaced. Never fails, never returns the input
    /// unchanged when it contains a registered secret.
    fn redact(&self, text: &str) -> String;
    /// True when `text` holds a registered secret. The check on the way out; cheaper than `redact`.
    fn contains_secret(&self, text: &str) -> bool;
}
pub trait KeyedDigest: Send + Sync {
    /// A digest under a key the caller never sees, so that equal inputs can be matched without the
    /// digest revealing them to someone who guesses. `purpose` keeps two uses from ever colliding.
    fn keyed_digest(&self, purpose: DigestPurpose, bytes: &[u8]) -> Digest;
}
pub enum DigestPurpose { BlobName, RestrictedValue }

// To build: findings, as data. Spec section 58 defines every field.
pub struct Finding { /* id, subject, kind: FindingKind, found, level, source, coverage, limitations, derived_from, provider, observed_at, evidence */ }
pub enum Subject {
    File { digest: Digest, format: String, part: Option<String> },
    Address { origin: String, path: String },
    Process { id: u32, started_at: Instant, program: Digest },
    Session { session: String, attached_to: String },
    Device { reported_id: String, model: String, firmware: String },
    Unknown { reason: UnknownReason },
}
pub enum UnknownReason { Derived, NotObserved, NotSupported, Withheld }

// To build: the plan, as data. Produced only by the compiler in `a2m-files`; run by `a2m-engine`.
// No map that iterates in an unstable order, no float, no timestamp, no path from a developer's machine.
pub struct Plan {
    pub header: PlanHeader,                 // format, minimum engine, compiler version, target, version, digest of the sources
    pub surfaces: Vec<SurfacePlan>,         // name, SurfaceKind, address, session sharing, rate
    pub field_types: Vec<FieldTypePlan>,    // base, places, rounding, unit, zone rule, codes, sensitivity; normalizer as an ExprIndex
    pub expressions: Vec<PlanExpr>,         // every expression once; referred to by ExprIndex
    pub operations: Vec<OperationPlan>,
    pub pipelines: Vec<PipelinePlan>,       // the step graph, checkpoints, limits, success checks, source and mapping
    pub tools: Vec<ToolPlan>,               // names, descriptions, schemas, annotations, as the front doors list them
    pub packs: Vec<PackSection>,            // each pack's own compiled knowledge, opaque here
    pub tests: Vec<TestPlan>,
}
pub struct PlanExpr { pub source: String, pub result: ValueType, pub library: u32 }   // canonical CEL text, already checked
pub struct ExprIndex(u32);

pub struct OperationPlan {
    pub name: OperationName, pub effect: EffectClass, pub hazard: Hazard,
    pub inputs: Vec<InputPlan>, pub outputs: SchemaRef, pub identity: IdentityPlan, pub key: Vec<InputIndex>,
    pub commit_points: u32,
    pub bindings: Vec<BindingPlan>,         // declared order: the order `choose` tries them
    pub manifest: PermissionManifest,       // surfaces, uploads, downloads, destinations
}
pub struct BindingPlan {
    pub mode: Mode, pub verb: Verb, pub status: BindingStatus, pub surface: SurfaceIndex,
    pub act: Vec<Step>, pub confirm: Vec<Step>, pub read_back: Vec<Step>,
    pub precondition: Option<ExprIndex>,
}

/// One fixed instruction, carried out by the driver of its binding's mode. The only name for a
/// compiled step; the technical reference's `CompiledStep` is this type.
pub struct Step {
    pub kind: StepKind,
    pub refers_to: Option<PackRef>,         // a screen field, an action, an endpoint, inside a PackSection
    pub args: Vec<(ArgName, ExprIndex)>,    // the engine evaluates these and hands the driver `Values`
    pub timeout_ms: u32,
    pub fails_as: (FailureClass, FailureCode),
    pub commits: bool,                      // a save in the application: one ledger row
    pub when: Option<ExprIndex>,
    pub payload: PackPayload,
}

/// A pack's own data, encoded by the pack with the plan's encoding. Stored and hashed here; decoded
/// only by the pack.
pub struct PackSection { pub pack: String, pub kind: FileKindName, pub format: u16, pub payload: PackPayload }
pub struct PackPayload(Vec<u8>);
impl PackPayload {
    pub fn encode<T: Serialize>(value: &T) -> Result<Self, Failure>;
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T, Failure>;
}

impl Plan {
    pub fn encode(&self) -> Result<EncodedPlan, Failure>;                       // a versioned envelope; the hash over the body
    pub fn decode(bytes: &[u8], expected: &Digest) -> Result<Plan, Failure>;   // checks the hash before anything else
    pub fn show(&self) -> String;                                               // stable text, for `plan.show`
}
pub struct EncodedPlan { pub bytes: Vec<u8>, pub hash: Digest }
```

Contracts that the signatures do not show:

- **A vocabulary's names are the documents' names.** `tests/documents.rs` reads the documents and fails on any difference, including order. `Availability`, `SubsystemState`, `SubsystemMove`, and `DoctorState` join it when they are built.
- **A value that is not stated takes the weakest claim.** `EvidenceLevel` defaults to `inferred`, `Coverage` to `unknown`, and a `Finding` built without a level or coverage gets those. An availability check that cannot be made is `unknown`, never `ready`.
- **A finding's level is `inferred` or `discovered` and nothing higher.** The constructor refuses the rest of the ladder; those levels belong to drafts and versions.
- **A finding's `id` is a digest of its content,** computed here, so that the same observation has the same identifier on every machine. Display names and local paths are left out of it.
- **An `Address` subject carries no credentials.** The constructor refuses an origin with user information in it.
- **`Decimal` is exact.** Which outside crate supplies it is settled, with its licence checked, by the slice that first compares two business values.
- **No type here can hold a secret except `Secret`.** `Value` has no secret variant: a secret is never a field's value.
- **A checked name says nothing about whether it is registered.** `StepKind::new("locate")` succeeds whatever packs are enabled. Whether anyone registered it is the compiler's question and the registry's, and an unregistered one does not compile.
- **A plan is immutable data.** Nothing here builds one from target files; the compiler in `a2m-files` is the only producer, and tests the only other builders. The engine receives a plan only through `decode`, so it never runs bytes whose hash it has not checked.
- **The same plan encodes to the same bytes.** On every machine and every run. `Plan::encode` and `PackPayload::encode` use one deterministic encoding; a pack cannot choose another for its payload.
- **A plan holds expressions as text with their type.** `a2m-expr` turns its compiled form into a `PlanExpr` and back. This crate never parses one.

## Used by, and uses

Every crate uses it. It uses nothing in the workspace.

| Caller | Uses |
| --- | --- |
| `a2m-files` | Names and checked names, `Value`, `EffectClass`, `Hazard`, `BindingStatus`, `Failure`; builds and encodes the `Plan` |
| `a2m-expr` | `Value`, `Failure`; converts its compiled form to and from `PlanExpr` |
| `a2m-ledger` | States, `RunId`, `RecordKey`, `Digest`, `Finding`, `Redact`, `KeyedDigest` for blob names; stores plan bytes without decoding them |
| `a2m-telemetry` | `Redact`, `KeyedDigest` for restricted values, `StepKind`, identifiers |
| `a2m-policy` | `EffectClass`, `Hazard`, `Identity`, `Failure` |
| `a2m-auth` | `SessionState`, `Identity`, `Secret` |
| `a2m-provider` and packs | `Archetype`, the checked names, `Availability`, `SubsystemState`, `SubsystemMove`, `Finding`, `Subject`, `Value`, `Failure`, `Secret`; packs build `Step` and `PackPayload` in their lowerings and decode their payloads in their drivers |
| `a2m-engine` | All of it; the `Plan` through `decode` |
| `a2m-actions` | `DoctorState` for `doctor.run`; `Plan::show` for `plan.show` |
| `a2m-vault` | `Secret`; implements `Redact` and `KeyedDigest` |

## State

None.

## Failures

| Class | Code | When |
| --- | --- | --- |
| `input` | `schema_invalid` | A name is not one of a vocabulary's values, or is badly formed. The next step lists what would have been accepted |
| `input` | `value_out_of_range` | A decimal, a date, or a key part cannot be represented |
| `input` | `plan_hash_mismatch` | A plan's bytes do not match the hash it was published with. That tool is unavailable ([section 52](../../spec/52-when-the-engine-itself-fails.md)) |
| `input` | `plan_format_unsupported` | A plan's format is neither the current one nor the one before |
| `input` | `payload_unreadable` | A pack's payload does not decode as the type its driver expects |

It produces no other class. A crate that needs a new code defines it itself, as a `FailureCode` constant.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Code and documents agree on every vocabulary, in content and order | `tests/documents.rs`, thirteen cases today; four more with the provider and supervisor states |
| A secret is never shown by any formatter, alone or inside another structure | `secret::tests` |
| `Secret` is neither serializable nor copyable | A test that fails to compile otherwise |
| A badly formed name is refused, and the refusal never echoes more than 64 characters of input | `name::tests`, extended to each new checked name |
| Two findings with the same content have the same `id`; a change to any content field changes it | To build, with B2 |
| A finding cannot be constructed above `discovered` | To build, with B2 |
| Comparing two `Value`s never involves a float | To build; the type has no float variant |
| A plan whose bytes differ from its hash by one bit is refused before any field is read | A test that flips each byte of a small plan in turn |
| A plan encodes to the same bytes twice, and on three operating systems | A round-trip test in CI on each system, comparing hashes |
| No plan type can hold a float or an unordered map | A test that encodes a plan through a serializer that fails on either |

## Build order

1. Done: vocabularies, `Mode`, `FailureCode`, `Failure`, `Secret`.
2. `Digest`, with the SHA-256 crate verified; `Subject`, `UnknownReason`, `FindingKind`, `Finding`, with the identifier rule and the level rule. Package B2.
3. `Availability`, held to section 56 by the documents test. Package B3, the manifest's first user.
4. The checked identifiers `TargetName`, `OperationName`, `SurfaceName`, `Environment`, and `SurfaceKind`. Package B4, their first user.
5. `Plan` and its parts, `Step`, `StepKind`, `Verb`, `PackPayload`, with a provisional encoding. Package 1.1, whose hand-written plan is the first.
6. `FileKindName`, `LintCode`, and the encoding chosen by measurement. Package 1.3, with the compiler.
7. `Value`, `Values`, `RecordKey`, and the choice of decimal crate. With package 1.1, the first read, or 3.4 if 1.1 reads only text.
8. `Redact` with `contains_secret`, and `KeyedDigest`. Package 2.1, with the redactor.
9. `Identity`, `RunId`. With the first slice of `a2m-ledger`.
10. `SubsystemState`, `SubsystemMove`, `DoctorState`. With the supervisor in `a2m-provider`.

## Open points

- The decimal crate, and whether dates and instants come from the standard library, one outside crate, or a small type of our own.
- Whether `OperationName` enforces the four-part form or only the shape of a name; the compiler can check the parts either way.
- The closed lists section 58 still owes: limitation codes, which become an enum here. Finding kinds are a checked name, because packs bring their own.
- Whether `Values` keeps declared order or sorts by name. Order matters to trace signatures, so it must be one or the other everywhere.
- **Whether `Verb` should stay a closed enum.** For: a binding's verb decides how the engine treats it (a read cannot commit, an `upsert` needs an identity strategy), the technical reference fixes the verbs in one table, and a closed list stops synonyms such as `put` and `save` from spreading. Against: packs beyond the web bring verbs of their own, such as subscribing to a stream or setting a device object, and a closed list makes the kernel change for each. The checked name is the default, with the engine's treatment declared by the lowering rather than read from the verb's spelling. Package 1.1 decides.
- `ValueType`, the type stored with each `PlanExpr`, would replace the expression type sketched in `a2m-expr`, so that the two crates share one description of a type.
- The plan's encoding is a second outside crate here. If the slice that chooses it prefers to keep this crate to `serde` and SHA-256, `decode` takes the deserializer as an argument instead.
- Whether `DoctorState` and `Availability` should be one list. They overlap in three names and answer different questions: can this run here, and is this working now.
- Whether `KeyedDigest` is keyed per installation or per workspace. Per workspace is safer, and makes blob names differ between workspaces.
