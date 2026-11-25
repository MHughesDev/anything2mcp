# `a2m-core`

The words every other crate shares: the closed vocabularies, the checked names that arrive with packs, identifiers, values, failures, findings, the secret wrapper, and the compiled target as data. It holds types and the rules that make a value well formed. It does nothing.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 1, Types |
| May depend on | Nothing internal |
| Depends on | Outside: `serde`; `serde_json`, for the compiled target's encoding; `sha2`, for `Digest`, the compiled target's hash, and keyed digests; `rust_decimal`, wrapped by `Decimal` and named by no other crate ([decision 0068](../../decisions/0068-values-names-and-identifiers-as-the-core-crate-settles-them.md)); `zeroize`, which wipes a dropped `Secret` ([decision 0122](../../decisions/0122-small-things-the-core-crate-gives-its-builders-and-a-secret-is-wiped-when-dropped.md)) |
| Roadmap packages | Stage B. Every step of the build order below is built |
| Carries out | Spec sections [105](../../spec/1-product/105-core-concepts.md), [207](../../spec/2-kernel/207-the-engine-loop.md), [212](../../spec/2-kernel/212-policy-and-approvals.md), [208](../../spec/2-kernel/208-runs-the-ledger-and-resume.md), [206](../../spec/2-kernel/206-the-compiler-and-the-compiled-target.md) (what the compiled target contains, and its encoding), [201](../../spec/2-kernel/201-access-kinds-and-modes.md), [203](../../spec/2-kernel/203-the-provider-contract.md) (availability), [204](../../spec/2-kernel/204-supervising-subsystems.md) (subsystem states), [209](../../spec/2-kernel/209-record-identity-and-dependent-records.md) (record matchings, the record key), [210](../../spec/2-kernel/210-sources-and-value-normalization.md) (values), [218](../../spec/2-kernel/218-discovery-and-findings.md), [219](../../spec/2-kernel/219-the-evidence-ladder.md), [213](../../spec/2-kernel/213-effects-hazards-and-consent.md); the [core types](../../reference/technical-reference.md#core-types); [215](../../spec/2-kernel/215-sessions-and-the-session-keeper.md) (what a sign-in is made of, as data); decisions [0036](../../decisions/0036-target-files-are-compiled-into-a-hashed-plan-before-anything-runs.md), [0068](../../decisions/0068-values-names-and-identifiers-as-the-core-crate-settles-them.md), [0069](../../decisions/0069-the-plan-is-canonical-json-hashed-whole-and-keyed-digests-are-per-workspace.md), [0121](../../decisions/0121-the-plan-carries-what-the-run-time-needs-from-target-files.md), and [0122](../../decisions/0122-small-things-the-core-crate-gives-its-builders-and-a-secret-is-wiped-when-dropped.md) |

## Owns

- Every word with a fixed set of values, as an enum whose names are the documents' names.
- The shape of every name that arrives from outside: a mode, a surface kind, a step kind, a verb, a file kind, a lint code, a finding kind, a control name, a failure code, a target, an operation, a surface, an environment, an account.
- The opaque identifiers the store gives out, and `Identity`: who a system sees.
- The value model: what a field's value can be once it has been read from a file, a caller, or a system; its type; exact decimals; dates.
- `Failure`, the one form in which anything in the kernel reports that it could not do what was asked.
- `Finding` and its `Subject`: what discovery returns, as data.
- `Secret`, and the two interfaces the vault implements: `Redact` and `KeyedDigest`, with the one construction of a keyed digest.
- The `CompiledTarget` and its `Step`, as immutable data: their one encoding, and the check of a compiled target's hash.
- `AddressScope` and `ScopedHost`: what kind of place an address is, read from its literal alone, and the one question every check of a plain link asks, whether a link to it may leave this machine ([decision 0240](../../decisions/0240-one-typed-reading-of-where-an-address-is-and-one-question-of-a-plain-link.md)).

It must never: perform input or output; read a clock or a random source; hold a list of modes, packs, or providers; contain logic that differs by kind of system; compile a compiled target, evaluate an expression, or decode a pack's payload; depend on another crate in this workspace.

## Interface

All of it is built. Every name below is exported from the crate's root; a crate above imports each by its bare name from there, and no module path is public.

```rust
// Closed vocabularies: each has ALL, NAMES, as_str, Display, FromStr, and serde by name.
pub enum AccessKind { Interface, Request, Command, Bridge, File, Stream, Device }   // File is written `artifact` (decision 0402)
pub enum EffectClass { Read, Write, Irreversible }            // ordered
pub enum Hazard { None, DataElsewhere, SendsOutside, DeviceFunction, Physical }   // ordered
pub enum FailureClass { /* twelve */ }
pub enum EvidenceLevel { /* seven, ordered; default Inferred */ }
pub enum WriteEvidence { ReadBack, Response, Acknowledged }    // what a write binding offers as evidence; default ReadBack; decision 0161
pub enum SurfaceNotice { UnencryptedCredentials }              // what a door must show beside a surface; words() fixed; decision 0193
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

// Target files and the compiled target.
pub enum RecordMatching { Natural, Composite, Stamped, Find, None }      // section 209
pub enum Sensitivity { Public, Internal, Restricted }          // ordered; default Internal
pub enum FieldBase { Text, Integer, Decimal, Money, Measurement, Date, Datetime, Time, Boolean, Choice }
pub enum EnvironmentKind { Test, Production }
pub enum TestKind { Offline, Live }
pub enum Checkpoint { PerRecord, PerStep }
pub enum SignInKind { UseExisting, ManagedSession, Custom }   // section 215
pub enum DriftRisk { Low, Medium, High }                       // ordered
pub enum CaptureLevel { Minimal, Standard, Full }              // section 217; ordered
pub enum ApproveWrites { CallerConfirms, PersonEachRun, NamedApprover }   // section 226; ordered, least strict first
pub enum WhitespaceRule { Collapse, Trim, Exact }
pub enum DecimalMark { Point /* "." */, Comma /* "," */ }
pub enum Rounding { HalfUp, HalfEven, HalfDown, Up, Down, Ceiling, Floor }
pub enum DigestPurpose { BlobName, RestrictedValue, RecordKey, CallerKey }

// Checked names: lowercase letters, digits, and underscores, starting with a letter, and never a
// name Windows keeps for a device (con, prn, aux, nul, com0-9, lpt0-9; decision 0431). Each has
// LONGEST, new, as_str, Display, FromStr, and serde that refuses a badly formed name. Each one
// made by the macro also has `const fn fixed(&'static str)`: in a `const`, a bad name fails the build.
checked_name! { /// Any crate declares one of its own.
    pub struct QueueName: "queue", 32; }                       // or `: "lint code", 64, dotted;` for owner.rule
pub struct Mode(/* 32 */);            pub struct FailureCode(/* 64; `fixed` for constants */);
pub struct FindingKind(/* 32 */);     pub struct SurfaceKind(/* 32 */);
pub struct StepKind(/* 32 */);        pub struct Verb(/* 32 */);
pub struct FileKindName(/* 32 */);    pub struct ControlName(/* 32 */);
pub struct TargetName(/* 32 */);      pub struct SurfaceName(/* 32 */);
pub struct Environment(/* 32 */);     pub struct AccountName(/* 64 */);
pub struct OperationName(/* 64 */);   impl OperationName { pub fn belongs_to(&self, target: &TargetName) -> bool; }
pub struct AuthProviderName(/* 32 */);                         // such as api_key; `a2m-auth` names it `ProviderName`
pub struct CredentialRef(/* vault:// and one to four parts; a refusal never shows the text */);   // SCHEME, new, as_str
pub struct LintCode(/* owner and rule joined by one dot, 64 in all: kernel.no_fixed_wait */);
impl LintCode { pub fn owner(&self) -> &str; pub fn rule(&self) -> &str; }
pub struct InvalidName; pub struct UnknownName;               // both convert to Failure
impl InvalidName { pub fn next_step(&self) -> String; pub fn kept_for_a_device(&self) -> Option<&str>; }

pub struct Failure { /* class, code, message, next_step; and `said`, what the system itself said, whole and untrusted */ }
// Failure::with_said(text), said(), carrying_said(&other): kept apart from the sentence, never joined into it; decision 0162
// with_said keeps the text whole up to SAID_CEILING (64 KiB), above it SAID_TOO_LONG in its place: nothing is cut,
// joined, or flattened before the redactor sees it. said_cut / said_shown(redacted, SAID_LONGEST) cut it for a door,
// never inside a redaction mark, and said_shown escapes control characters; decision 0185
// said_hides(c) names every character that can reorder, hide, or restyle shown text: controls and terminal escapes,
// bidirectional marks and isolates, invisible format characters, line and paragraph separators, variation selectors
// and tags. said_cut keeps the text raw; it is escaped once, where a door shows it: said_shown (a line of its own),
// said_quoted (between quotes: quotes and backslashes too), said_neutral (inside JSON: all but line feed and tab).
// A Failure read as data is held to the same SAID_CEILING; decision 0193
// Failure::with_next(action, args) / next() -> Option<&NextAction { action, args }>: a way out as data;
// with_retry_after(Duration) / retry_after_ms(): a wait from when the failure was made, rounded up to whole ms.
// Both are written only when present; `said` and both sit behind one pointer, so Failure does not grow. Decision 0258
//
// The table form (decision 0258). Each crate calls, once, in `pub mod codes`:
//   failure_codes! { NAME = Class, "code", meaning "...", next "..."
//                    [, passes by_itself | not_by_itself] [, then ("noun.verb", "what for")]; ... }
// It writes a module `row` with a CodeEntry constant per row, the FailureCode constants of the same names
// (meaning and next step as their docs), ALL: &[(FailureClass, &str)], and TABLE: &[CodeEntry]. An empty
// meaning or next step, a bad code or action, and a `busy` row with no `passes` fail to compile.
// A failure of a code is made from its row, codes::row::NAME.failure(message, next), so the place that makes
// it cannot give it another class; a step's from what its compiled target says it fails as. Decision 0289
// Hidden, and built only for tests and under the feature `testing` (turned on as a feature of a dev-dependency):
// testing::classes_of_their_own(src, remakes) -> io::Result<Vec<String>>, what each crate's test
// `every_failure_takes_its_class_from_a_table` reads its `src` with. It skips what `#[cfg(test)]` marks (a module,
// inline or in its own file or folder, and any single item, field, statement, or arm) and reads the code after it;
// `remakes` names each file that remakes a failure in the class it has, with its exact count. The one place this
// crate reads a file
pub struct CodeEntry { pub declared_in, pub class, pub code, pub meaning, pub next, pub then: Option<ThenAction>, pub passes: Passes }
impl CodeEntry { pub const fn failure_code(&self) -> FailureCode; pub fn failure(&self, message, next) -> Failure; }
impl FailsAs { pub fn failure(&self, message, next) -> Failure; }   // and From<&CodeEntry> for FailsAs
pub struct ThenAction { pub action: &'static str, pub when: &'static str }
pub enum Passes { ByItself, NotByItself }                       // default_for(class): only `transient` passes by itself
pub struct CodeBook;                                             // new(&[TABLE, ...]) refuses one code with two classes
impl CodeBook { pub fn explain(&self, code: &str) -> Option<&'static CodeEntry>; pub fn passes(&self, code: &str, class: FailureClass) -> Passes; pub fn entries(&self); }
pub struct Secret(/* prints as [redacted]; no Serialize, no Clone; wiped when dropped */);
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
pub struct FieldHasher;                                          // new, field(name, value), finish: the digest of named fields
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
    pub fn with_places(&self, places: u32) -> Option<Self>;     // the same number padded to places; None, never rounded, when it has more
    pub fn exact_text(&self) -> String;                         // no zeros after the last digit, whatever places it holds
}
pub struct Date(/* a day that exists, years 1 to 9999 */);       // new, year, month, day, to_days, from_days; text 2026-11-17
pub struct Millis(i64);                                         // a length of time; may be negative

// Redaction and keyed digests. The vault implements both.
pub trait Redact: Send + Sync {
    fn redact(&self, text: &str) -> String;                      // the one required method
    fn contains_secret(&self, text: &str) -> bool;               // default: redact(text) != text
    fn redact_failure(&self, failure: &Failure) -> Failure;      // default
    fn redact_value(&self, value: &Value) -> Value;              // default; a number, a date, a moment, and a length of time by their text
    fn redact_values(&self, values: &Values) -> Values;          // default; names too, and two names that redact alike are both kept
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

// The compiled target, as data. Produced only by the compiler in `a2m-files`; run by `a2m-engine`.
// No map that iterates in an unstable order, no float, no timestamp, no path from a developer's machine.
pub const COMPILED_TARGET_FORMAT: u16 = 1;
pub struct CompiledTarget {
    pub header: CompiledTargetHeader,                 // format, minimum_engine, compiler, target, version, sources
    pub target: TargetSection,                 // what holds for the target as a whole; below
    pub surfaces: Vec<CompiledSurface>,         // name, kind, address, same_session_as, stands, rate, programs, payload
    pub environments: Vec<CompiledEnvironment>, // name, kind, addresses, test_records: Vec<RecordKey>, approve_writes: Option<ApproveWrites>
    pub auth: Vec<CompiledAuth>,                // one per group of surfaces that share a sign-in; below
    pub field_types: Vec<CompiledFieldType>,    // name, base, ty, places, rounding, step, unit, display_unit, currency, zone, codes, sensitivity, untrusted, format, normalizer
    pub expressions: Vec<CompiledExpr>,         // every expression once; referred to by ExprIndex
    pub operations: Vec<CompiledOperation>,
    pub pipelines: Vec<CompiledPipeline>,       // name, effect, hazard, inputs, source, limits, steps, success, headroom_ms
    pub tools: Vec<CompiledTool>,               // name, runs, module, title, description, input_schema, output_schema, annotations
    pub packs: Vec<PackSection>,            // each pack's own compiled knowledge, opaque here
    pub tests: Vec<CompiledTest>,               // name, tool, kind, fixture, inputs, expect, restore
}
/// A program a surface's driver may start. `CompiledSurface::programs` lists them for every environment of the target;
/// the engine hands them to the supervisor and reads nothing else in them. A path that is not absolute does not encode or decode.
pub struct DeclaredProgram { pub path: String, pub digest: Option<Digest> }
impl DeclaredProgram {
    pub fn is_absolute_path(path: &str) -> bool;   // from the root, a drive, or a network share; the same answer on every machine
    pub fn is_direct_path(path: &str) -> bool;     // no part is `.` or `..`, with either slash
    pub fn is_share_path(path: &str) -> bool;      // absolute, and on a network share
}
pub struct CompiledExpr { pub source: String, pub result: ValueType, pub library: u32 }   // canonical CEL text, already checked
pub struct ExprIndex(u32);   // and SurfaceIndex, EnvironmentIndex, FieldTypeIndex, InputIndex, OperationIndex, PipelineIndex, SectionIndex, ToolIndex: new, get

pub struct TargetSection {                     // from `target.yaml`; nothing here belongs to one kind of surface
    pub title: String, pub modules: Vec<String>,
    pub default_environment: EnvironmentIndex,
    pub locale: Option<String>, pub timezone: Option<String>,   // `UTC` or an offset such as `+02:00`
    pub capture: Option<CaptureLevel>,
    pub exclusive_session: bool, pub drift_risk: Option<DriftRisk>,
    pub authorization: Option<String>,      // a reference to the record, never the record
}
pub enum Stands { Alone, Beneath(SurfaceIndex) }

pub struct CompiledAuth {                       // one entry of `auth.yaml`
    pub surfaces: Vec<SurfaceIndex>,        // no surface is in two entries
    pub mode: SignInKind, pub provider: AuthProviderName,
    pub probe: CompiledSessionCheck,
    pub expiry_signals: Vec<PackPayload>,   // each in the form the surface's pack defines
    pub idle_ms: Option<u64>, pub absolute_ms: Option<u64>,
    pub sign_out: Option<String>,           // a name the provider resolves
    pub settings: Vec<AuthSetting>,         // name, value: text; sorted by name
    pub credentials: Vec<CredentialUse>,    // role, reference: CredentialRef; sorted by role
    pub attempt_limit: u32, pub movable: bool,
}
pub enum CompiledSessionCheck {
    Operation { operation: OperationIndex, inputs: Values },   // a read, run through the engine
    Entry(PackRef),                                            // an entry of a pack section, such as an endpoint
}

pub enum FieldFormat {                      // how the system shows a value; data beside the normalizer
    Text { case_sensitive: bool, whitespace: WhitespaceRule },
    Number { decimal_mark: DecimalMark },
    Calendar { pattern: String, utc_offset_minutes: Option<i32> },
    Boolean { true_text: String, false_text: String },
    Choice,
}

pub struct CompiledOperation {
    pub name: OperationName, pub effect: EffectClass, pub hazard: Hazard,
    pub inputs: Vec<CompiledInput>,             // name, field_type, ty, required, default, description
    pub outputs: Vec<CompiledOutput>,           // name, ty, field_type
    pub identity: CompiledIdentity,             // strategy, key: Vec<InputIndex>
    pub commit_points: u32,
    pub bindings: Vec<CompiledBinding>,         // declared order: the order `choose` tries them
    pub manifest: PermissionManifest,       // surfaces, uploads, downloads, destinations
    pub examples: Vec<Values>,              // example inputs, each typed by the inputs
    pub decision_points: Vec<RunQuestion>,   // name, question
}
pub struct CompiledBinding {
    pub mode: Mode, pub verb: Verb, pub status: BindingStatus, pub surface: SurfaceIndex,
    pub act: Vec<Step>, pub confirm: Vec<Step>, pub read_back: Vec<Step>,
    pub lookup: Vec<Step>, pub settle_ms: u32,      // for the `stamped` and `find` strategies
    pub evidence: WriteEvidence,            // read_back, response, or acknowledged (only for an irreversible operation); decision 0161
                                            // and never where acknowledgement_may_stand(strategy, read_back) is false:
                                            // a record read back by its key is verified by that read; decision 0193
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
pub struct CompiledSource { pub template: String, pub reader: String, pub mapping: Vec<StepArg>, pub rows: Option<ExprIndex>, pub payload: PackPayload }
pub enum ToolRuns { Operation(OperationIndex), Pipeline(PipelineIndex) }
pub enum ZoneRule { Fixed, User, Named(String) }

/// A pack's own data, in the compiled target's encoding. Stored and hashed here; decoded only by the pack.
pub struct PackSection { pub pack: String, pub kind: FileKindName, pub surface_kind: SurfaceKind, pub format: u16, pub payload: PackPayload }
pub struct CanonicalJson(/* canonical JSON text */);   pub type PackPayload = CanonicalJson;
impl CanonicalJson {
    pub fn encode<T: Serialize + ?Sized>(value: &T) -> Result<Self, Failure>;
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T, Failure>;
    pub fn none() -> Self;   pub fn is_none(&self) -> bool;   pub fn as_bytes(&self) -> &[u8];   pub fn digest(&self) -> Digest;
}

impl CompiledTarget {
    pub fn encode(&self) -> Result<EncodedCompiledTarget, Failure>;                       // one first line, then canonical JSON; checks references
    pub fn decode(bytes: &[u8], expected: &Digest) -> Result<CompiledTarget, Failure>;   // checks the hash before anything else
    pub fn check(&self) -> Result<(), Failure>;                                 // the format, every reference, and the rules of a write
    pub fn show(&self) -> Result<String, Failure>;                              // stable text, for `target.show`
    pub fn expression(&self, index: ExprIndex) -> Option<&CompiledExpr>;            // and surface, environment, field_type, operation, pipeline, section
    pub fn default_environment(&self) -> Option<&CompiledEnvironment>;              // and environment_named(&Environment)
    pub fn auth_of(&self, surface: SurfaceIndex) -> Option<&CompiledAuth>;          // follows same_session_as
    pub fn tool(&self, name: &OperationName) -> Option<&CompiledTool>;
    pub fn section_of(&self, kind: &FileKindName, surface_kind: &SurfaceKind) -> Option<&PackSection>;
}
pub struct EncodedCompiledTarget { pub bytes: Vec<u8>, pub hash: Digest }                 // the hash is over all the bytes
```

Where an address is, read from its literal (decision 0240):

```rust
pub enum AddressScope {   // this_machine, unspecified, link_local, private_use, carrier_grade_nat, unique_local,
                          // multicast, documentation, reserved, public, name, unreadable
}
pub struct ScopedHost { /* scope, IP address, name as written, IPv4 inside IPv6, zone */ }
impl ScopedHost {
    pub fn of_ip(ip: IpAddr) -> Self;                          // IPv4 inside IPv6 is read as IPv4, and remembered
    pub fn of_socket_address(address: SocketAddr) -> Self;     // a scope identifier other than 0 is a zone
    pub fn of_host(host: &str) -> Self;                        // a URL's host: dotted IPv4, IPv6 bare or in brackets, or a name
    pub fn of_destination(address: &str) -> Self;              // a URL, host:port, [v6]:port, or a bare host
    pub fn scope(&self) -> AddressScope;
    pub fn ip(&self) -> Option<IpAddr>;
    pub fn name(&self) -> Option<&str>;
    pub fn written_as_ipv4_in_ipv6(&self) -> bool;
    pub fn written_with_zone(&self) -> bool;
    pub fn may_leave_this_machine(&self) -> bool;              // false only for a loopback literal, written plainly
}
```

Contracts that the signatures do not show:

- **A vocabulary's names are the documents' names.** `tests/documents.rs` reads the documents and fails on any difference, including order. `Rounding` and `DigestPurpose` are this crate's own lists; this document is where they are written.
- **A value that is not stated takes the weakest claim.** `EvidenceLevel` defaults to `inferred`, `Coverage` to `unknown`, and a `Finding` built without a level or coverage gets those. An availability check that cannot be made is `unknown`, never `ready`, and `DoctorState` defaults to `unknown` in the same way.
- **A finding's level is `inferred` or `discovered` and nothing higher.** The constructor refuses the rest of the ladder; those levels belong to drafts and versions.
- **A finding's `id` is a digest of its content,** computed here, so that the same observation has the same identifier on every machine. Display names and local paths are left out of it.
- **An `Address` subject carries no credentials.** The constructor refuses an origin with user information in it.
- **`Decimal` is exact.** Two decimals are equal when they are the same number, hash alike, and give the same canonical bytes, from their exact text. A decimal is written with the places it holds, which are its exact places unless `with_places` padded it or its stored form had more, so a result padded to its field type's places keeps them through the store ([decision 0444](../../decisions/0444-a-results-decimal-is-padded-to-the-places-its-field-type-states.md)); `CompiledFieldType::result_places` says which places those are. A sum, a difference, or a product is exact or it is refused; the wrapper never rounds to make a result fit. A division takes places and a rounding mode and is the true quotient correctly rounded.
- **Kinds of value are never converted.** A whole number does not equal a decimal, and text does not equal a number. A choice is the text of its stored code.
- **`Values` holds its names sorted.** The same named values are equal, encode to the same bytes, and have the same digest, whatever order they were added in. The order a person sees comes from the compiled target's schema.
- **No type here can hold a secret except `Secret`.** `Value` has no secret variant: a secret is never a field's value.
- **A checked name says nothing about whether it is registered.** `StepKind::new("locate")` succeeds whatever packs are enabled. Whether anyone registered it is the compiler's question and the registry's, and an unregistered one does not compile.
- **The engine never reads a verb's spelling.** A binding is treated by what its steps and its operation state: `commits`, `lookup`, `idempotent`, the effect class, the record matching.
- **Nothing here reads a clock or a random source.** `RunId::from_parts` takes the moment and the random bytes. Whoever makes an identifier brings both.
- **A scope is read from the text alone.** `ScopedHost` resolves no name and asks the machine nothing. `this_machine` is a loopback literal, never a name. A spelling only a lenient parser widens (`127.1`), malformed text, and a path are `unreadable`. The ranges are those the Rust core library's source classifies, with the standard it cites for each; `scope::tests` holds each range at its edges.
- **A plain link stays on this machine only to a loopback address written plainly.** `may_leave_this_machine` is true for a name, `localhost` included, for IPv4 written inside IPv6, and for an address with a zone. `a2m-auth`, the HTTP, MCP, stream, and data packs each ask it after their own parser, and `tests/address-spellings.txt` pins each of them, spelling by spelling.
- **A compiled target is immutable data.** Nothing here builds one from target files; the compiler in `a2m-files` is the only producer, and tests the only other builders. The engine receives a compiled target only through `decode`, so it never runs bytes whose hash it has not checked.
- **The same compiled target encodes to the same bytes.** On every machine and every run. `CompiledTarget::encode`, `CanonicalJson::encode`, and `Value::canonical_bytes` use one deterministic encoding, canonical JSON: no spaces, keys sorted by their bytes, whole numbers only. A pack cannot choose another for its payload. A float is refused.
- **A compiled target holds no credential.** A sign-in's credentials are `CredentialRef`s, which have one shape and nothing else; bytes that hold other text in a reference's place do not decode, and the refusal does not show the text.
- **A field type's format and its normalizer say the same thing.** The compiler makes both from one rule. The format is for writing a value and describing the type; only the normalizer decides whether two values are equal.
- **The compiled target names no kind of surface.** What one kind has, such as a viewport, is in that surface's payload. An expiry signal is a payload for the same reason.
- **A compiled target's references are checked on the way out and on the way in.** `encode` and `decode` both refuse an index that points at a part the compiled target does not hold, so the engine indexes a decoded compiled target without checking again.
- **A compiled target keeps the rules of a write, whoever made it** ([decision 0153](../../decisions/0153-what-the-compiler-the-plan-and-the-redactor-hold-by-themselves-after-the-safety-audit.md)). `check` refuses, on the way out and on the way in: an operation with a hazard that is not `irreversible`; a `write` whose record matching is `none`; a read that states a commit point or has a step that commits; a write with no commit point, or with a binding whose committing steps among `act` are not exactly its commit points; a step that commits in `confirm`, `read_back`, or `lookup`; a `write`, or an `irreversible` operation with an identity, with a binding that has no read-back; a pipeline whose effect or hazard is below one of its operations'; and a tool described as read-only that runs anything but a read. The hash proves the bytes are the ones stored. These rules are what make them safe to run. The refusal is `compiled_target_unreadable` and names the operation by its position, never its content. Two of the compiler's rules are not held here: that a sign-in's probe is a read, and that a pipeline that writes has a record limit and a checkpoint.
- **A declared program is named directly.** Beside being absolute, a path holds no `.` or `..` part, and a program on a network share is declared with its digest; `check` refuses the rest. The second supersedes one line of [decision 0124](../../decisions/0124-the-plan-declares-a-surfaces-programs-and-a-schema-checks-run-time-values.md), which let a share path stand alone.
- **A secret that is a number is redacted as a number's text.** `redact_value` judges a whole number, a decimal, a date, a moment, and a length of time by the text each is written as, and replaces one that holds a registered secret by the text `[redacted]`. What that costs: a short numeric secret, such as a four-digit PIN, also removes every number whose digits hold it; the value's kind becomes text, so a result's schema must allow text where a number may be redacted; and each number in a result is one more search. What it does not find: a secret written with leading zeros, or with zeros after a decimal mark, in a number that has lost them; and a secret that lies across two values.
- **A compiled target holds expressions as text with their type.** `a2m-expr` turns its compiled form into a `CompiledExpr` and back. This crate never parses one.
- **The minimum engine version is the engine's to check.** `decode` cannot know which engine is running.
- **Every implementation of `KeyedDigest` gives the same digest for the same key,** because each calls `keyed_digest_under`. The key is separate for each workspace.

## Used by, and uses

Every crate uses it. It uses nothing in the workspace.

| Caller | Uses |
| --- | --- |
| `a2m-files` | Names and checked names, `Value`, `ValueType`, `EffectClass`, `Hazard`, `BindingStatus`, `RecordMatching`, `Failure`; builds and encodes the `CompiledTarget` |
| `a2m-expr` | `Value`, `ValueType`, `Decimal`, `Rounding`, `Date`, `Failure`; converts its compiled form to and from `CompiledExpr` |
| `a2m-ledger` | States, the identifiers, `RecordKey`, `Digest`, `Finding`, `Redact`, `KeyedDigest` for blob names and record keys; stores compiled target bytes without decoding them |
| `a2m-logs` | `Redact`, `KeyedDigest` for restricted values, `Sensitivity`, `StepKind`, identifiers |
| `a2m-policy` | `EffectClass`, `Hazard`, `Identity`, `Sensitivity`, the identifiers, `Failure` |
| `a2m-auth` | `SessionState`, `Identity`, `SessionId`, `WorkspaceId`, `Secret`, `AuthProviderName`, `CredentialRef`; `a2m-actions` builds the keeper's entry from an `CompiledAuth` |
| `a2m-sched` | `RunId`, `JobId`, `RunnerId`, `Epoch`, `Mode`, `Identity`, `FailureClass`, `FailureCode` |
| `a2m-provider` and packs | `AccessKind`, the checked names, `Availability`, `SubsystemState`, `SubsystemMove`, `Finding`, `Subject`, `Value`, `Values`, `Failure`, `Secret`; packs build `Step` and `PackPayload` in their lowerings and decode their payloads in their drivers |
| `a2m-engine` | All of it; the `CompiledTarget` through `decode` |
| `a2m-actions` | `DoctorState` for `doctor.run`; `CompiledTarget::show` for `target.show` |
| `a2m-vault` | `Secret`, `WorkspaceId`; implements `Redact` and `KeyedDigest`, the second through `keyed_digest_under` |

## State

None.

## Failures

| Class | Code | When |
| --- | --- | --- |
| `input` | `schema_invalid` | A name is not one of a vocabulary's values, or a name, a digest, an identifier, a version, or an identity is badly formed. The next step lists what would have been accepted |
| `input` | `finding_invalid` | A finding claims more than its source, its parents, or its content allow |
| `input` | `value_out_of_range` | A decimal, a date, a key part, or the moment in an identifier cannot be represented |
| `input` | `compiled_target_hash_mismatch` | A compiled target's bytes do not match the hash it was published with. That tool is unavailable ([section 224](../../spec/2-kernel/224-when-the-engine-itself-fails.md)) |
| `input` | `compiled_target_format_unsupported` | A compiled target's format is neither the current one nor the one before |
| `input` | `compiled_target_unreadable` | A compiled target's bytes match its hash and are not a compiled target that holds together: not this format's shape, a field it does not have, a reference to a part that is not there, a program named indirectly, or an operation that breaks a rule of a write |
| `input` | `payload_unreadable` | A payload does not decode as the type its reader expects |
| `input` | `payload_not_encodable` | Data to be encoded holds a float, a map whose keys are not text, or a number beyond 64 bits |

It produces no other class. The codes are declared in this crate's own `failure_codes!` table, the module `codes`, and are also associated constants of `FailureCode`, such as `FailureCode::SCHEMA_INVALID`. A crate that needs a new code declares it in its own table ([decision 0258](../../decisions/0258-every-crate-declares-its-failure-codes-in-one-table.md)). `CodeBook::new` refuses a code with two classes as `schema_invalid`. No message here carries a value: a refusal names a kind, a place, or a count.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Code and documents agree on every vocabulary, in content and order | `tests/documents.rs`, 34 cases |
| A secret is never shown by any formatter, alone or inside another structure | `secret::tests` |
| `Secret` is neither serializable nor copyable | A test that fails to compile otherwise |
| A table row with an empty meaning or next step, or a `busy` row that does not say whether it passes by itself, does not compile; a book refuses one code with two classes | `code_book::tests`; two `compile_fail` doc tests of `failure_codes!` |
| The reading of the class test skips only what `#[cfg(test)]` marks, reads the code after it, skips the files of a test module, and fails a file whose remakes are not the stated count | `testing::tests` |
| A failure with no next action, wait, or text from the system is written as before, keeps the others through serde, and is no larger for them | `failure::tests` |
| A badly formed name is refused with a failure of class `input`, and the refusal never echoes more than 64 characters of input | `name::tests`, `ident::tests` over every checked name |
| Two findings with the same content have the same `id`; a change to any content field changes it | `finding::tests` |
| A finding cannot be constructed above `discovered` | `finding::tests` |
| Comparing two `Value`s never involves a float, and a float is not a value | The type has no float variant; `value::tests` refuses one on the way in |
| Decimal comparison and arithmetic are exact, and a result that does not fit is refused, never rounded | `decimal::tests`, with a quotient a hair above a tie |
| The same named values have the same bytes and the same digest in whatever order they were added | `value::tests` |
| A compiled target whose bytes differ from its hash by one bit is refused before any field is read | `compiled_target::tests`, which flips a bit of every byte of a small compiled target in turn, and every bit of its first 64 bytes |
| A compiled target encodes to the same bytes twice, and on three operating systems | `compiled_target::tests`, which holds the hash of a sample compiled target to a constant, on each system in CI |
| A compiled target decodes to an equal compiled target, and encodes back to the same bytes | `compiled_target::tests` |
| No compiled target type is written as a float, and no map's own order reaches the bytes | `compiled_target::tests` walks an encoded compiled target that uses every part; `canonical::tests` encodes a hash map twenty times and refuses a float |
| A compiled target with a loose reference is refused by `encode` and by `decode` | `compiled_target::tests`, one case for each kind of reference, and one for a surface signed in to twice |
| A credential in a compiled target is a reference; other text in its place is refused and never shown | `compiled_target::tests`, `ident::tests` |
| A program a compiled target declares is named by an absolute path, judged the same way on every machine, with no `.` or `..` in it, and with a digest when it is on a network share; a compiled target that names one any other way is refused by `encode` and by `decode` | `compiled_target::tests` |
| A compiled target that breaks a rule of a write is refused by `encode` and by `decode`: a write stored as a read, a write with no commit point or no read-back, a commit outside the steps that do the work | `compiled_target::tests`, one case for each rule, and bytes changed under their own hash |
| A name written in the source is checked when the crate is built | `ident::tests`, whose constants would not compile otherwise; the doc test of `checked_name!` |
| A dropped secret's bytes are zeros | `secret::tests`, on what `Drop` calls; memory that was given back cannot be read by a test |
| The digest of named fields has one encoding | `digest::tests`, against a digest worked out apart from the crate |
| The trait's own helpers redact a failure and values at every depth, names included | `redact::tests`, with a redactor that implements `redact` alone |
| A secret a system returns as a number, a date, a moment, or a length of time does not pass as one; two names that redact alike keep both values | `redact::tests` |
| A keyed digest differs by key, by workspace, and by purpose, and HMAC gives the standard answers | `redact::tests`, with three cases of RFC 4231 |
| An identifier reads back what it writes, sorts by the moment it was made, and refuses another kind's prefix | `ident::tests` |
| Each address range holds exactly its standard's addresses; IPv4 inside IPv6 is read as IPv4; only a loopback literal written plainly stays on this machine; a name is kept and never resolved | `scope::tests`; `tests/address_spellings.rs`, over the shared corpus |

## Build order

1. Done: vocabularies, `Mode`, `FailureCode`, `Failure`, `Secret`.
2. Done: `Digest`, on the `sha2` crate; `Subject`, `Address`, `UnknownReason`, `Source`, `LimitationCode`, `FindingKind`, `Timestamp`, and `Finding` with its builder, the identifier rule, and the level rules.
3. Done: `Availability`, held to section 203 by the documents test.
4. Done: the checked identifiers `TargetName`, `OperationName`, `SurfaceName`, `Environment`, `SurfaceKind`, and with them `ControlName` and `AccountName`.
5. Done: `CompiledTarget` and its parts, `Step`, `StepKind`, `Verb`, `PackPayload`; `RecordMatching`, `Sensitivity`, `FieldBase`, `EnvironmentKind`, `TestKind`, `Checkpoint`, each held to its document.
6. Done: `FileKindName`, `LintCode`, and the encoding: canonical JSON, format 1 ([decision 0069](../../decisions/0069-the-plan-is-canonical-json-hashed-whole-and-keyed-digests-are-per-workspace.md)). Not done: measuring compiled target loading against its budget, which needs a compiled target of real size.
7. Done: `Value`, `Values`, `RecordKey`, `ValueType`, `Decimal` on `rust_decimal`, `Rounding`, `Date`, `Millis` ([decision 0068](../../decisions/0068-values-names-and-identifiers-as-the-core-crate-settles-them.md)).
8. Done: `Redact` with `contains_secret` and its helpers, `NoSecrets`, `KeyedDigest`, `keyed_digest_under`.
9. Done: `Identity`, `RunId`, and the other identifiers the store, the scheduler, and policy share; `Epoch`; `Version`.
10. Done: `SubsystemState`, `SubsystemMove`, `DoctorState`.
11. Done: what the run time needs from target files and the compiled target lacked ([decision 0121](../../decisions/0121-the-plan-carries-what-the-run-time-needs-from-target-files.md)): `TargetSection`, `CompiledAuth` with `CompiledSessionCheck`, `AuthProviderName`, and `CredentialRef`, `Stands`, an environment's test records and approval rule, an operation's examples and run questions, a tool's module, `FieldFormat` and a field type's step; with `SignInKind`, `DriftRisk`, `CaptureLevel`, `ApproveWrites`, `WhitespaceRule`, and `DecimalMark`, each held to its document.
12. Done: the public `checked_name!` with `fixed`, `FieldHasher`, and `Secret` wiped when dropped ([decision 0122](../../decisions/0122-small-things-the-core-crate-gives-its-builders-and-a-secret-is-wiped-when-dropped.md)).
13. Done: `DeclaredProgram` and `CompiledSurface::programs`, the programs a surface's driver may start, in the compiled target's bytes and under its hash ([decision 0124](../../decisions/0124-the-plan-declares-a-surfaces-programs-and-a-schema-checks-run-time-values.md)).
14. Done: `AddressScope` and `ScopedHost`, the reading of where an address is and the one question of a plain link ([decision 0240](../../decisions/0240-one-typed-reading-of-where-an-address-is-and-one-question-of-a-plain-link.md)). Not done: a place in the sense of [decision 0223](../../decisions/0223-a-network-is-a-place-a-recognized-network-and-a-reach.md), which needs the machine's own state.
15. Done: the table form of failure codes, `failure_codes!`, with `CodeEntry`, `ThenAction`, `Passes`, and `CodeBook`; this crate's own codes in it; and `Failure`'s next action and wait as data ([decision 0258](../../decisions/0258-every-crate-declares-its-failure-codes-in-one-table.md)). Nothing reads the two new parts yet.
16. Done: the module `row` of each table, `CodeEntry::failure`, `FailsAs::failure`, and `FailsAs` from a row, so that a failure takes its class from its code's row ([decision 0289](../../decisions/0289-a-failure-is-made-from-its-codes-row.md)).
17. Done: the hidden module `testing` and its feature, one reading of a crate's sources for the eleven copies of `every_failure_takes_its_class_from_a_table`. It replaced a reading that stopped at a file's first `#[cfg(test)]`, and so missed the code after a test-only item in nine files, and that skipped three files whole where they remake a failure; those now have an exact count each. No call site was found that the old reading had missed.

## Open points

- The compiled target's shape changed once more for decision 0161: a binding's `prefer_response` is now its `evidence`, and the sample's pinned hash moved with it. The format number stays 1, because no release has carried a compiled target: a compiled target stored before the change does not load and is compiled again.
- The compiled target's parts have met the compiler and not yet an engine. The engine's slice confirms each one or changes it. The format number is still 1 because no release has carried a compiled target; from the first release on, a change that stored compiled targets cannot be read under raises it.
- `CaptureLevel` and `ApproveWrites` are here because the compiled target carries them. `a2m-logs` and `a2m-policy` each have a list of their own with the same names, which should become these.
- A run question is a name and a question, which is all the file formats say. The engine's design of `needs_input` may add what a person chooses between.
- `Mode` and `FindingKind` are written by hand and have no `fixed`. They can be declared with `checked_name!` when a pack wants a constant of one.
- The long form of `identity` for `stamped` and `find` is not fixed in the file formats. The compiled target carries a `lookup` step list and a settle time per binding for it; slice 3.8 confirms.
- `PackSection::pack` and `ProducedBy::provider` are plain text. A provider's name becomes a checked name when the manifest's shape is built in `a2m-provider`.
- Whether compiled target loading as JSON meets the 20 ms target of section 206. If not, a binary encoding is format 2 behind the same two functions.
- A float that is not a number, held by a pack's payload type, is written by the outside crate as nothing and so is not refused when encoding; it is refused when the pack decodes. No kernel type can hold one.
- `ValueType` has no unknown member. If drafting needs one, it is added here, as the expression design says.
- Whether `Diff` and its canonical form belong here, so that the approval surface can render one without the engine. The engine design asks.
