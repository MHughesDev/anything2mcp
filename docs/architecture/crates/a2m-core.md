# `a2m-core`

The words every other crate shares: the closed vocabularies, identifiers, values, failures, findings, and the secret wrapper. It holds types and the rules that make a value well formed. It does nothing.

## Facts

| | |
| --- | --- |
| Side | Kernel |
| Layer | 1, Types |
| May depend on | Nothing internal |
| Depends on | Outside: `serde` |
| Built in | Stage B, packages B1 and B2. The vocabularies, `Failure`, and `Secret` exist; the rest is to build |
| Carries out | Spec sections [5](../../spec/05-core-concepts.md), [8](../../spec/08-execution-engine.md), [18](../../spec/18-safety-policy-and-approvals.md), [42](../../spec/42-run-ledger-design.md), [54](../../spec/54-interaction-archetypes.md), [58](../../spec/58-discovery-and-findings.md), [59](../../spec/59-the-evidence-ladder.md), [61](../../spec/61-effects-hazards-and-consent.md); the [core types](../../reference/technical-reference.md#core-types) |

## Owns

- Every word with a fixed set of values, as an enum whose names are the documents' names.
- The shape of every name that arrives from outside: a mode, a failure code, a target, an operation.
- The value model: what a field's value can be once it has been read from a file, a caller, or a system.
- `Failure`, the one form in which anything in the kernel reports that it could not do what was asked.
- `Finding` and its `Subject`: what discovery returns, as data.
- `Secret`, and the `Redact` interface that the vault implements.

It must never: perform input or output; hold a list of modes, packs, or providers; contain logic that differs by kind of system; depend on another crate in this workspace.

## Interface

A sketch. Built items are marked; the rest is settled by package B2 and the slices that first need each type.

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

// Built. Checked names whose values arrive with packs and crates.
pub struct Mode(/* [a-z][a-z0-9_]*, at most 32 */);
pub struct FailureCode(/* the same shape, at most 64 */);

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

// To build: redaction. The vault implements it; everything that emits text takes one.
pub trait Redact: Send + Sync {
    /// Returns `text` with every registered secret replaced. Never fails, never returns the input
    /// unchanged when it contains a registered secret.
    fn redact(&self, text: &str) -> String;
}

// To build: findings, as data. Spec section 58 defines every field.
pub struct Finding { /* id, subject, kind, found, level, source, coverage, limitations, derived_from, provider, observed_at, evidence */ }
pub enum Subject {
    File { digest: Digest, format: String, part: Option<String> },
    Address { origin: String, path: String },
    Process { id: u32, started_at: Instant, program: Digest },
    Session { session: String, attached_to: String },
    Device { reported_id: String, model: String, firmware: String },
    Unknown { reason: UnknownReason },
}
pub enum UnknownReason { Derived, NotObserved, NotSupported, Withheld }
```

Contracts that the signatures do not show:

- **A vocabulary's names are the documents' names.** `tests/documents.rs` reads the documents and fails on any difference, including order.
- **A value that is not stated takes the weakest claim.** `EvidenceLevel` defaults to `inferred`, `Coverage` to `unknown`, and a `Finding` built without a level or coverage gets those.
- **A finding's level is `inferred` or `discovered` and nothing higher.** The constructor refuses the rest of the ladder; those levels belong to drafts and versions.
- **A finding's `id` is a digest of its content,** computed here, so that the same observation has the same identifier on every machine. Display names and local paths are left out of it.
- **An `Address` subject carries no credentials.** The constructor refuses an origin with user information in it.
- **`Decimal` is exact.** Which outside crate supplies it is settled, with its licence checked, by the slice that first compares two business values.
- **No type here can hold a secret except `Secret`.** `Value` has no secret variant: a secret is never a field's value.

## Used by, and uses

Every crate uses it. It uses nothing in the workspace.

| Caller | Uses |
| --- | --- |
| `a2m-files` | Names, `Value`, `EffectClass`, `Hazard`, `BindingStatus`, `Mode`, `Failure` for lint and compile errors |
| `a2m-ledger` | States, `RunId`, `RecordKey`, `Digest`, `Finding`, `Redact` |
| `a2m-policy` | `EffectClass`, `Hazard`, `Identity`, `Failure` |
| `a2m-auth` | `SessionState`, `Identity`, `Secret` |
| `a2m-provider` and packs | `Archetype`, `Mode`, `Finding`, `Subject`, `Value`, `Failure`, `Secret` |
| `a2m-engine` | All of it |
| `a2m-vault` | `Secret`; implements `Redact` |

## State

None.

## Failures

| Class | Code | When |
| --- | --- | --- |
| `input` | `schema_invalid` | A name is not one of a vocabulary's values, or is badly formed. The next step lists what would have been accepted |
| `input` | `value_out_of_range` | A decimal, a date, or a key part cannot be represented |

It produces no other class. A crate that needs a new code defines it itself, as a `FailureCode` constant.

## Invariants

| Must always hold | Proven by |
| --- | --- |
| Code and documents agree on every vocabulary, in content and order | `tests/documents.rs`, thirteen cases today |
| A secret is never shown by any formatter, alone or inside another structure | `secret::tests` |
| `Secret` is neither serializable nor copyable | A test that fails to compile otherwise |
| A badly formed name is refused, and the refusal never echoes more than 64 characters of input | `name::tests` |
| Two findings with the same content have the same `id`; a change to any content field changes it | To build, with B2 |
| A finding cannot be constructed above `discovered` | To build, with B2 |
| Comparing two `Value`s never involves a float | To build; the type has no float variant |

## Build order

1. Done: vocabularies, `Mode`, `FailureCode`, `Failure`, `Secret`.
2. `Digest`, `Subject`, `UnknownReason`, `Finding`, with the identifier rule and the level rule. Package B2.
3. The checked identifiers: `TargetName`, `OperationName`, `SurfaceName`, `Environment`. With package B4, which is their first user.
4. `Redact`. With the first slice of `a2m-vault`.
5. `Value`, `Values`, `RecordKey`, and the choice of decimal crate. With slice 1.1, the first read.
6. `Identity`, `RunId`. With the first slice of `a2m-ledger`.

## Open points

- The decimal crate, and whether dates and instants come from the standard library, one outside crate, or a small type of our own.
- Whether `OperationName` enforces the four-part form or only the shape of a name; the compiler can check the parts either way.
- The closed lists section 58 still owes: finding kinds and limitation codes. They become enums here when the findings store is built.
- Whether `Values` keeps declared order or sorts by name. Order matters to trace signatures, so it must be one or the other everywhere.
