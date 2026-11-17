# `a2m-core`

The words every other crate shares: the closed vocabularies, the checked names that arrive with packs, identifiers, values, failures, findings, the secret wrapper, and the compiled plan as data. It holds types and the rules that make a value well formed. It does nothing.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 1, Types |
| May depend on | Nothing internal |
| Depends on | Outside: `serde`; `serde_json`, for the plan's encoding; `sha2`, for `Digest`, the plan's hash, and keyed digests; `rust_decimal`, wrapped by `Decimal` and named by no other crate ([decision 0068](../../decisions/0068-values-names-and-identifiers-as-the-core-crate-settles-them.md)) |
| Built in | Stage B. Every step of the build order below is built |
| Carries out | Spec sections [105](../../spec/1-product/105-core-concepts.md), [207](../../spec/2-kernel/207-the-engine-loop.md), [212](../../spec/2-kernel/212-policy-and-approvals.md), [208](../../spec/2-kernel/208-runs-the-ledger-and-resume.md), [206](../../spec/2-kernel/206-the-compiler-and-the-plan.md) (what the plan contains, and its encoding), [201](../../spec/2-kernel/201-archetypes-and-modes.md), [203](../../spec/2-kernel/203-the-provider-contract.md) (availability), [204](../../spec/2-kernel/204-supervising-subsystems.md) (subsystem states), [209](../../spec/2-kernel/209-record-identity-and-dependent-records.md) (identity strategies, the record key), [210](../../spec/2-kernel/210-sources-and-value-normalization.md) (values), [218](../../spec/2-kernel/218-discovery-and-findings.md), [219](../../spec/2-kernel/219-the-evidence-ladder.md), [213](../../spec/2-kernel/213-effects-hazards-and-consent.md); the [core types](../../reference/technical-reference.md#core-types); decisions [0036](../../decisions/0036-target-files-are-compiled-into-a-hashed-plan-before-anything-runs.md), [0068](../../decisions/0068-values-names-and-identifiers-as-the-core-crate-settles-them.md), and [0069](../../decisions/0069-the-plan-is-canonical-json-hashed-whole-and-keyed-digests-are-per-workspace.md) |

## Owns

- Every word with a fixed set of values, as an enum whose names are the documents' names.
- The shape of every name that arrives from outside: a mode, a surface kind, a step kind, a verb, a file kind, a lint code, a finding kind, a control name, a failure code, a target, an operation, a surface, an environment, an account.
- The opaque identifiers the store gives out, and `Identity`: who a system sees.
- The value model: what a field's value can be once it has been read from a file, a caller, or a system; its type; exact decimals; dates.
- `Failure`, the one form in which anything in the kernel reports that it could not do what was asked.
- `Finding` and its `Subject`: what discovery returns, as data.
- `Secret`, and the two interfaces the vault implements: `Redact` and `KeyedDigest`, with the one construction of a keyed digest.
- The `Plan` and its `Step`, as immutable data: their one encoding, and the check of a plan's hash.

It must never: perform input or output; read a clock or a random source; hold a list of modes, packs, or providers; contain logic that differs by kind of system; compile a plan, evaluate an expression, or decode a pack's payload; depend on another crate in this workspace.

## Interface

All of it is built. Every name below is exported from the crate's root; a crate above imports each by its bare name from there, and no module path is public.

```rust
// Closed vocabularies: each has ALL, NAMES, as_str, Display, FromStr, and serde by name.
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
pub enum Source { /* five */ }           pub enum LimitationCode { /* eight */ }   pub enum UnknownReason { /* four */ }

// The provider contract and the supervisor.
pub enum Availability {                                        // section 203; default Unknown
    Ready, NotEnabled, ComponentMissing, ComponentUnverified,
    UnsupportedHost, UnsupportedVersion, Misconfigured, Unknown,
}
impl Availability { pub const fn is_ready(self) -> bool; }     // only Ready
pub enum SubsystemState { Idle, Busy, Exited, Unreachable }     // section 204
pub enum SubsystemMove { Retry, Wait, Restart }
pub enum DoctorState {                                         // section 204; default Unknown
    Healthy, ComponentMissing, RuntimeFailure, UnsupportedHost, UnsupportedTarget, ConfigDrift, Unknown,
}

// Target files and the plan.
pub enum IdentityStrategy { Natural, Composite, Stamped, Find, None }      // section 209
pub enum Sensitivity { Public, Internal, Restricted }          // ordered; default Internal
pub enum FieldBase { Text, Integer, Decimal, Money, Measurement, Date, Datetime, Time, Boolean, Choice }
pub enum EnvironmentKind { Test, Production }
pub enum TestKind { Offline, Live }
pub enum Checkpoint { PerRecord, PerStep }
pub enum Rounding { HalfUp, HalfEven, HalfDown, Up, Down, Ceiling, Floor }
pub enum DigestPurpose { BlobName, RestrictedValue, RecordKey }

// Checked names: lowercase letters, digits, and underscores, starting with a letter. Each has
// LONGEST, new, as_str, Display, FromStr, and serde that refuses a badly formed name.
pub struct Mode(/* 32 */);            pub struct FailureCode(/* 64; `fixed` for constants */);
pub struct FindingKind(/* 32 */);     pub struct SurfaceKind(/* 32 */);
pub struct StepKind(/* 32 */);        pub struct Verb(/* 32 */);
pub struct FileKindName(/* 32 */);    pub struct ControlName(/* 32 */);
pub struct TargetName(/* 32 */);      pub struct SurfaceName(/* 32 */);
pub struct Environment(/* 32 */);     pub struct AccountName(/* 64 */);
pub struct OperationName(/* 64 */);   impl OperationName { pub fn belongs_to(&self, target: &TargetName) -> bool; }
pub struct LintCode(/* owner and rule joined by one dot, 64 in all: kernel.no_fixed_wait */);
impl LintCode { pub fn owner(&self) -> &str; pub fn rule(&self) -> &str; }
pub struct InvalidName; pub struct UnknownName;               // both convert to Failure

pub struct Failure { /* class, code, message, next_step */ }
pub struct Secret(/* prints as [redacted]; no Serialize, no Clone */);
impl Secret { pub fn expose(&self) -> &str; }

// Opaque identifiers: 16 bytes, time-ordered, written as a prefix, an underscore, and 32 hex characters.
pub struct RunId; pub struct JobId; pub struct SessionId; pub struct ApprovalId;      // r_ j_ s_ a_
pub struct DecisionId; pub struct WorkspaceId; pub struct PrincipalId; pub struct RunnerId;   // d_ w_ p_ n_
impl RunId /* and each of the others */ {
    pub const PREFIX: &'static str;
    pub fn from_parts(at: Timestamp, random: [u8; 10]) -> Result<Self, InvalidId>;   // the caller brings the clock and the randomness
    pub const fn from_bytes(bytes: [u8; 16]) -> Self;   pub const fn as_bytes(&self) -> &[u8; 16];
    pub fn made_at(&self) -> Timestamp;
}
pub enum Identity { Person(PrincipalId), Account(AccountName) }   // text: person:p_… or account:name
pub struct Epoch(u64);                                           // a lease number; new, get, next
pub struct Version { pub major: u32, pub minor: u32, pub patch: u32 }   // text: 1.4.0
pub struct Digest(/* sha-256, as lowercase hex */);              // of, from_bytes, as_bytes
pub struct Timestamp(/* a moment: milliseconds since 1970 in UTC */);

// Values. What a field holds after it is read and before it is compared.
pub enum Value {
    Null, Bool(bool), Int(i64), Text(String),
    Decimal(Decimal),        // exact; never a float
    Instant(Timestamp),      // a moment, with its zone rule applied
    Date(Date),
    Duration(Millis),
    List(Vec<Value>),
    Record(Values),
}
impl Value {
    pub const fn kind(&self) -> &'static str;                    // for messages; never the value
    pub fn has_type(&self, ty: &ValueType) -> bool;
    pub fn canonical_bytes(&self) -> Vec<u8>;                    // and as_text, as_bool, as_int, as_decimal, as_list, as_record
}
pub struct Values(/* field name to Value, sorted by name */);
impl Values {
    pub const fn new() -> Self;
    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<Value>) -> Option<Value>;
    pub fn with(self, name: impl Into<String>, value: impl Into<Value>) -> Self;
    pub fn get(&self, name: &str) -> Option<&Value>;             // and remove, contains, len, is_empty, iter, names
    pub fn canonical_bytes(&self) -> Vec<u8>;   pub fn digest(&self) -> Digest;
}
pub struct RecordKey(/* one or more plain values, in declared order */);
impl RecordKey { pub fn new(parts: Vec<Value>) -> Result<Self, InvalidValue>; pub fn parts(&self) -> &[Value]; pub fn canonical_bytes(&self) -> Vec<u8>; }
pub enum ValueType { Null, Bool, Int, Text, Decimal, Instant, Date, Duration, Choice, List(Box<ValueType>), Record(Vec<FieldOf>) }
pub struct FieldOf { pub name: String, pub ty: ValueType, pub required: bool }

pub struct Decimal(/* up to 28 significant digits */);
impl Decimal {
    pub fn parse(text: &str) -> Result<Self, InvalidValue>;      // digits and one point; no exponent
    pub fn from_i64(number: i64) -> Self;   pub fn from_units(units: i64, places: u32) -> Result<Self, InvalidValue>;
    pub fn checked_add(&self, other: &Self) -> Option<Self>;     // exact or None; also checked_sub, checked_mul
    pub fn checked_div(&self, divisor: &Self, places: u32, rounding: Rounding) -> Option<Self>;
    pub fn round(&self, places: u32, rounding: Rounding) -> Self;
    pub fn to_places(&self, places: u32, rounding: Rounding) -> String;   // also places, to_i64, abs, negated, is_zero, is_negative
}
pub struct Date(/* a day that exists, years 1 to 9999 */);       // new, year, month, day, to_days, from_days; text 2026-11-17
pub struct Millis(i64);                                         // a length of time; may be negative

// Redaction and keyed digests. The vault implements both.
pub trait Redact: Send + Sync {
    fn redact(&self, text: &str) -> String;                      // the one required method
    fn contains_secret(&self, text: &str) -> bool;               // default: redact(text) != text
    fn redact_failure(&self, failure: &Failure) -> Failure;      // default
    fn redact_value(&self, value: &Value) -> Value;              // default
    fn redact_values(&self, values: &Values) -> Values;          // default; names too
}
pub struct NoSecrets;                                            // nothing registered; for tests and where no vault can exist
pub trait KeyedDigest: Send + Sync {
    fn keyed_digest(&self, workspace: &WorkspaceId, purpose: DigestPurpose, bytes: &[u8]) -> Digest;
}
pub fn keyed_digest_under(key: &[u8], workspace: &WorkspaceId, purpose: DigestPurpose, bytes: &[u8]) -> Digest;
pub fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> Digest;

// Findings, as data. Spec section 218 defines every field.
pub struct Finding { /* id, subject, kind: FindingKind, found, level, source, coverage, limitations, derived_from, produced_by, observed_at, evidence */ }
pub enum Subject {
    File { digest: Digest, format: String, part: Option<String> },
    Address(Address),
    Process { id: u32, started_at: Timestamp, program: Digest },
    Session { session: String, attached_to: String },
    Device { reported_id: String, model: String, firmware: String },
    Unknown { reason: UnknownReason },
}

// The plan, as data. Produced only by the compiler in `a2m-files`; run by `a2m-engine`.
// No map that iterates in an unstable order, no float, no timestamp, no path from a developer's machine.
pub const PLAN_FORMAT: u16 = 1;
pub struct Plan {
    pub header: PlanHeader,                 // format, minimum_engine, compiler, target, version, sources
    pub surfaces: Vec<SurfacePlan>,         // name, kind, address, same_session_as, rate, payload
    pub environments: Vec<EnvironmentPlan>, // name, kind, addresses: Vec<AddressOverride>
    pub field_types: Vec<FieldTypePlan>,    // name, base, ty, places, rounding, unit, display_unit, currency, zone, codes, sensitivity, untrusted, normalizer
    pub expressions: Vec<PlanExpr>,         // every expression once; referred to by ExprIndex
    pub operations: Vec<OperationPlan>,
    pub pipelines: Vec<PipelinePlan>,       // name, effect, hazard, inputs, source, limits, steps, success, headroom_ms
    pub tools: Vec<ToolPlan>,               // name, runs, title, description, input_schema, output_schema, annotations
    pub packs: Vec<PackSection>,            // each pack's own compiled knowledge, opaque here
    pub tests: Vec<TestPlan>,               // name, tool, kind, fixture, inputs, expect, restore
}
pub struct PlanExpr { pub source: String, pub result: ValueType, pub library: u32 }   // canonical CEL text, already checked
pub struct ExprIndex(u32);   // and SurfaceIndex, FieldTypeIndex, InputIndex, OperationIndex, PipelineIndex, SectionIndex, ToolIndex: new, get

pub struct OperationPlan {
    pub name: OperationName, pub effect: EffectClass, pub hazard: Hazard,
    pub inputs: Vec<InputPlan>,             // name, field_type, ty, required, default, description
    pub outputs: Vec<OutputPlan>,           // name, ty, field_type
    pub identity: IdentityPlan,             // strategy, key: Vec<InputIndex>
    pub commit_points: u32,
    pub bindings: Vec<BindingPlan>,         // declared order: the order `choose` tries them
    pub manifest: PermissionManifest,       // surfaces, uploads, downloads, destinations
}
pub struct BindingPlan {
    pub mode: Mode, pub verb: Verb, pub status: BindingStatus, pub surface: SurfaceIndex,
    pub act: Vec<Step>, pub confirm: Vec<Step>, pub read_back: Vec<Step>,
    pub lookup: Vec<Step>, pub settle_ms: u32,      // for the `stamped` and `find` strategies
    pub prefer_response: bool,
    pub precondition: Option<ExprIndex>,
}

/// One fixed instruction, carried out by the driver of its binding's mode. The only name for a
/// compiled step; the technical reference's `CompiledStep` is this type.
pub struct Step {
    pub kind: StepKind,
    pub refers_to: Option<PackRef>,         // section: SectionIndex, entry: String
    pub args: Vec<StepArg>,                 // name, value: ExprIndex; the engine evaluates these and hands the driver `Values`
    pub timeout_ms: u32,
    pub fails_as: FailsAs,                  // class, code
    pub commits: bool,                      // a save in the application: one ledger row
    pub lookup: bool,                       // may report the record absent
    pub idempotent: bool,                   // the system takes the idempotency token
    pub when: Option<ExprIndex>,
    pub payload: PackPayload,
}
pub struct PipelineStep { pub id: String, pub operation: OperationIndex, pub with: Vec<StepArg>, pub for_each: Option<ExprIndex>,
                          pub checkpoint: Option<Checkpoint>, pub when: Option<ExprIndex>, pub expect: Option<ExprIndex> }
pub struct SourcePlan { pub template: String, pub reader: String, pub mapping: Vec<StepArg>, pub rows: Option<ExprIndex>, pub payload: PackPayload }
pub enum ToolRuns { Operation(OperationIndex), Pipeline(PipelineIndex) }
pub enum ZoneRule { Fixed, User, Named(String) }

/// A pack's own data, in the plan's encoding. Stored and hashed here; decoded only by the pack.
pub struct PackSection { pub pack: String, pub kind: FileKindName, pub surface_kind: SurfaceKind, pub format: u16, pub payload: PackPayload }
pub struct CanonicalJson(/* canonical JSON text */);   pub type PackPayload = CanonicalJson;
impl CanonicalJson {
    pub fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Self, Failure>;
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T, Failure>;
    pub fn none() -> Self;   pub fn is_none(&self) -> bool;   pub fn as_bytes(&self) -> &[u8];   pub fn digest(&self) -> Digest;
}

impl Plan {
    pub fn encode(&self) -> Result<EncodedPlan, Failure>;                       // one first line, then canonical JSON; checks references
    pub fn decode(bytes: &[u8], expected: &Digest) -> Result<Plan, Failure>;   // checks the hash before anything else
    pub fn check(&self) -> Result<(), Failure>;                                 // the format, and every reference
    pub fn show(&self) -> Result<String, Failure>;                              // stable text, for `plan.show`
    pub fn expression(&self, index: ExprIndex) -> Option<&PlanExpr>;            // and surface, field_type, operation, pipeline, section
    pub fn tool(&self, name: &OperationName) -> Option<&ToolPlan>;
    pub fn section_of(&self, kind: &FileKindName, surface_kind: &SurfaceKind) -> Option<&PackSection>;
}
pub struct EncodedPlan { pub bytes: Vec<u8>, pub hash: Digest }                 // the hash is over all the bytes
```

Contracts that the signatures do not show:

- **A vocabulary's names are the documents' names.** `tests/documents.rs` reads the documents and fails on any difference, including order. `Rounding` and `DigestPurpose` are this crate's own lists; this document is where they are written.
- **A value that is not stated takes the weakest claim.** `EvidenceLevel` defaults to `inferred`, `Coverage` to `unknown`, and a `Finding` built without a level or coverage gets those. An availability check that cannot be made is `unknown`, never `ready`, and `DoctorState` defaults to `unknown` in the same way.
- **A finding's level is `inferred` or `discovered` and nothing higher.** The constructor refuses the rest of the ladder; those levels belong to drafts and versions.
- **A finding's `id` is a digest of its content,** computed here, so that the same observation has the same identifier on every machine. Display names and local paths are left out of it.
- **An `Address` subject carries no credentials.** The constructor refuses an origin with user information in it.
- **`Decimal` is exact.** Two decimals are equal when they are the same number, and are written alike. A sum, a difference, or a product is exact or it is refused; the wrapper never rounds to make a result fit. A division takes places and a rounding mode and is the true quotient correctly rounded.
- **Kinds of value are never converted.** A whole number does not equal a decimal, and text does not equal a number. A choice is the text of its stored code.
- **`Values` holds its names sorted.** The same named values are equal, encode to the same bytes, and have the same digest, whatever order they were added in. The order a person sees comes from the plan's schema.
- **No type here can hold a secret except `Secret`.** `Value` has no secret variant: a secret is never a field's value.
- **A checked name says nothing about whether it is registered.** `StepKind::new("locate")` succeeds whatever packs are enabled. Whether anyone registered it is the compiler's question and the registry's, and an unregistered one does not compile.
- **The engine never reads a verb's spelling.** A binding is treated by what its steps and its operation state: `commits`, `lookup`, `idempotent`, the effect class, the identity strategy.
- **Nothing here reads a clock or a random source.** `RunId::from_parts` takes the moment and the random bytes. Whoever makes an identifier brings both.
- **A plan is immutable data.** Nothing here builds one from target files; the compiler in `a2m-files` is the only producer, and tests the only other builders. The engine receives a plan only through `decode`, so it never runs bytes whose hash it has not checked.
- **The same plan encodes to the same bytes.** On every machine and every run. `Plan::encode`, `CanonicalJson::encode`, and `Value::canonical_bytes` use one deterministic encoding, canonical JSON: no spaces, keys sorted by their bytes, whole numbers only. A pack cannot choose another for its payload. A float is refused.
- **A plan's references are checked on the way out and on the way in.** `encode` and `decode` both refuse an index that points at a part the plan does not hold, so the engine indexes a decoded plan without checking again.
- **A plan holds expressions as text with their type.** `a2m-expr` turns its compiled form into a `PlanExpr` and back. This crate never parses one.
- **The minimum engine version is the engine's to check.** `decode` cannot know which engine is running.
- **Every implementation of `KeyedDigest` gives the same digest for the same key,** because each calls `keyed_digest_under`. The key is separate for each workspace.

## Used by, and uses

Every crate uses it. It uses nothing in the workspace.

| Caller | Uses |
| --- | --- |
| `a2m-files` | Names and checked names, `Value`, `ValueType`, `EffectClass`, `Hazard`, `BindingStatus`, `IdentityStrategy`, `Failure`; builds and encodes the `Plan` |
| `a2m-expr` | `Value`, `ValueType`, `Decimal`, `Rounding`, `Date`, `Failure`; converts its compiled form to and from `PlanExpr` |
| `a2m-ledger` | States, the identifiers, `RecordKey`, `Digest`, `Finding`, `Redact`, `KeyedDigest` for blob names and record keys; stores plan bytes without decoding them |
| `a2m-telemetry` | `Redact`, `KeyedDigest` for restricted values, `Sensitivity`, `StepKind`, identifiers |
| `a2m-policy` | `EffectClass`, `Hazard`, `Identity`, `Sensitivity`, the identifiers, `Failure` |
| `a2m-auth` | `SessionState`, `Identity`, `SessionId`, `WorkspaceId`, `Secret` |
| `a2m-sched` | `RunId`, `JobId`, `RunnerId`, `Epoch`, `Mode`, `Identity`, `FailureClass`, `FailureCode` |
| `a2m-provider` and packs | `Archetype`, the checked names, `Availability`, `SubsystemState`, `SubsystemMove`, `Finding`, `Subject`, `Value`, `Values`, `Failure`, `Secret`; packs build `Step` and `PackPayload` in their lowerings and decode their payloads in their drivers |
| `a2m-engine` | All of it; the `Plan` through `decode` |
| `a2m-actions` | `DoctorState` for `doctor.run`; `Plan::show` for `plan.show` |
| `a2m-vault` | `Secret`, `WorkspaceId`; implements `Redact` and `KeyedDigest`, the second through `keyed_digest_under` |

## State

None.

## Failures

| Class | Code | When |
| --- | --- | --- |
| `input` | `schema_invalid` | A name is not one of a vocabulary's values, or a name, a digest, an identifier, a version, or an identity is badly formed. The next step lists what would have been accepted |
| `input` | `finding_invalid` | A finding claims more than its source, its parents, or its content allow |
| `input` | `value_out_of_range` | A decimal, a date, a key part, or the moment in an identifier cannot be represented |
| `input` | `plan_hash_mismatch` | A plan's bytes do not match the hash it was published with. That tool is unavailable ([section 224](../../spec/2-kernel/224-when-the-engine-itself-fails.md)) |
| `input` | `plan_format_unsupported` | A plan's format is neither the current one nor the one before |
| `input` | `plan_unreadable` | A plan's bytes match its hash and are not a plan that holds together: not this format's shape, a field it does not have, or a reference to a part that is not there |
| `input` | `payload_unreadable` | A payload does not decode as the type its reader expects |
| `input` | `payload_not_encodable` | Data to be encoded holds a float, a map whose keys are not text, or a number beyond 64 bits |

It produces no other class. Each code is a constant on `FailureCode`. A crate that needs a new code defines it itself, as a `FailureCode` constant. No message here carries a value: a refusal names a kind, a place, or a count.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Code and documents agree on every vocabulary, in content and order | `tests/documents.rs`, 27 cases |
| A secret is never shown by any formatter, alone or inside another structure | `secret::tests` |
| `Secret` is neither serializable nor copyable | A test that fails to compile otherwise |
| A badly formed name is refused with a failure of class `input`, and the refusal never echoes more than 64 characters of input | `name::tests`, `ident::tests` over every checked name |
| Two findings with the same content have the same `id`; a change to any content field changes it | `finding::tests` |
| A finding cannot be constructed above `discovered` | `finding::tests` |
| Comparing two `Value`s never involves a float, and a float is not a value | The type has no float variant; `value::tests` refuses one on the way in |
| Decimal comparison and arithmetic are exact, and a result that does not fit is refused, never rounded | `decimal::tests`, with a quotient a hair above a tie |
| The same named values have the same bytes and the same digest in whatever order they were added | `value::tests` |
| A plan whose bytes differ from its hash by one bit is refused before any field is read | `plan::tests`, which flips a bit of every byte of a small plan in turn, and every bit of its first 64 bytes |
| A plan encodes to the same bytes twice, and on three operating systems | `plan::tests`, which holds the hash of a sample plan to a constant, on each system in CI |
| A plan decodes to an equal plan, and encodes back to the same bytes | `plan::tests` |
| No plan type is written as a float, and no map's own order reaches the bytes | `plan::tests` walks an encoded plan that uses every part; `canonical::tests` encodes a hash map twenty times and refuses a float |
| A plan with a loose reference is refused by `encode` and by `decode` | `plan::tests`, one case for each kind of reference |
| The trait's own helpers redact a failure and values at every depth, names included | `redact::tests`, with a redactor that implements `redact` alone |
| A keyed digest differs by key, by workspace, and by purpose, and HMAC gives the standard answers | `redact::tests`, with three cases of RFC 4231 |
| An identifier reads back what it writes, sorts by the moment it was made, and refuses another kind's prefix | `ident::tests` |

## Build order

1. Done: vocabularies, `Mode`, `FailureCode`, `Failure`, `Secret`.
2. Done: `Digest`, on the `sha2` crate; `Subject`, `Address`, `UnknownReason`, `Source`, `LimitationCode`, `FindingKind`, `Timestamp`, and `Finding` with its builder, the identifier rule, and the level rules.
3. Done: `Availability`, held to section 203 by the documents test.
4. Done: the checked identifiers `TargetName`, `OperationName`, `SurfaceName`, `Environment`, `SurfaceKind`, and with them `ControlName` and `AccountName`.
5. Done: `Plan` and its parts, `Step`, `StepKind`, `Verb`, `PackPayload`; `IdentityStrategy`, `Sensitivity`, `FieldBase`, `EnvironmentKind`, `TestKind`, `Checkpoint`, each held to its document.
6. Done: `FileKindName`, `LintCode`, and the encoding: canonical JSON, format 1 ([decision 0069](../../decisions/0069-the-plan-is-canonical-json-hashed-whole-and-keyed-digests-are-per-workspace.md)). Not done: measuring plan loading against its budget, which needs a compiled plan of real size.
7. Done: `Value`, `Values`, `RecordKey`, `ValueType`, `Decimal` on `rust_decimal`, `Rounding`, `Date`, `Millis` ([decision 0068](../../decisions/0068-values-names-and-identifiers-as-the-core-crate-settles-them.md)).
8. Done: `Redact` with `contains_secret` and its helpers, `NoSecrets`, `KeyedDigest`, `keyed_digest_under`.
9. Done: `Identity`, `RunId`, and the other identifiers the store, the scheduler, and policy share; `Epoch`; `Version`.
10. Done: `SubsystemState`, `SubsystemMove`, `DoctorState`.

## Open points

- The plan's parts beyond the first operation are a first shape. `PipelinePlan`, `SourcePlan`, `TestPlan`, `ToolPlan`, and `PermissionManifest` have not met a compiler or an engine; the slice that first fills each one confirms it or changes it, and a change that old plans cannot be read under raises the format number.
- A plan holds an environment's kind and its address overrides, and nothing else about it. Test records and a stricter approval rule per environment are not in the plan; whether they belong there or with policy is settled by the slice that first writes in a test environment.
- The long form of `identity` for `stamped` and `find` is not fixed in the file formats. The plan carries a `lookup` step list and a settle time per binding for it; slice 3.8 confirms.
- `PackSection::pack` and `ProducedBy::provider` are plain text. A provider's name becomes a checked name when the manifest's shape is built in `a2m-provider`.
- Whether plan loading as JSON meets the 20 ms target of section 206. If not, a binary encoding is format 2 behind the same two functions.
- A float that is not a number, held by a pack's payload type, is written by the outside crate as nothing and so is not refused when encoding; it is refused when the pack decodes. No kernel type can hold one.
- `ValueType` has no unknown member. If drafting needs one, it is added here, as the expression design says.
- Whether `Diff` and its canonical form belong here, so that the approval surface can render one without the engine. The engine design asks.
