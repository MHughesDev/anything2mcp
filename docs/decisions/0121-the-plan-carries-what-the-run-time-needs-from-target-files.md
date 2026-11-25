# 0121. The plan carries what the run time needs from target files

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the list "What the plan cannot yet carry" of [decision 0079](0079-the-compilers-registration-points-and-the-file-formats-as-the-code-settles-them.md); two open points of the [`a2m-core` design](../architecture/crates/a2m-core.md), on an environment's test records and approval rule and on the first shape of the plan's parts; and where the source definition's types live. Changes the plan, which is a public contract, and several file formats.

## Context

The compiler validated and then dropped all of `auth.yaml` and a dozen fields of `target.yaml` and of an operation, because the plan had no place for them. The session keeper, policy, the engine, and the tool surface each need some of them at run time, and a published version must pin them. A field type's formatting facts lived only inside its normalizer's text, where a driver that types a value cannot read them. The source definition's types existed twice, in `a2m-files` and in `a2m-sources`.

## Decision

**The rule.** A fact from a target file is in the plan when the engine, the keeper, policy, or the tool surface reads it at run time. A fact that only changes what the compiler emits is applied by the compiler and is not in the plan. The engine applies no default and resolves no name.

**What the plan gains.**

- **`Plan::target`, a `TargetPlan`:** title, modules, default environment, locale, time zone, capture level, whether the system allows one session per account, drift risk, and the reference to the authorization record. The default environment is an index, `production` when the file names none.
- **`Plan::auth`, one `AuthPlan` per entry of `auth.yaml`.** The kernel's fields are typed: the surfaces, the sign-in mode, the provider as a checked name, the probe, declared timeouts in milliseconds, the attempt limit, whether the session may be moved. The provider's settings are names and text, sorted by name. A credential is a role and a `CredentialRef`, a name the vault resolves; no plan type can hold the value, and bytes that hold anything else in its place do not decode. Expiry signals are opaque payloads, one each, because their shape belongs to the pack of the surface. `Plan::auth_of` finds the entry of a surface, following `same_session_as`. No surface is covered twice.
- **The probe is one of two things.** A `read` operation of the target, with the inputs it is called with, which the layer above the keeper runs through the engine as any read; or an entry of a pack's file kind on one of the entry's surfaces, such as an endpoint, as a `PackRef`. Any other name does not compile. The new key `probe_inputs` gives an operation's inputs, and every required input must be given, so a probe can always be run as written.
- **An environment** gains its test records, each a `RecordKey` as the file wrote it, and its approval rule. Policy applies the stricter of that rule and the installation's own.
- **A surface** gains `stands`: alone, or beneath another surface.
- **An operation** gains its example inputs, typed by its inputs, and its decision points, each a name and a question. **A tool** gains its module.
- **A field type** gains `format`, a `FieldFormat`: for text, the case and whitespace rules; for a number, the decimal mark; for a date or a time, the pattern and, where the compiler could fix the zone, its offset from UTC in minutes; for a boolean, the two texts. It also gains `step`. The compiler makes the format and the normalizer from one rule, so they cannot disagree. The normalizer stays the one rule by which two values are compared.

**What stays out of the plan.**

- **`defaults.timeouts` and `defaults.limits`** are inputs to compiling. A timeout reaches a lowering through `TargetView::default_timeout_ms`, and the lowering writes it into the step. A limit goes to a pipeline that states none. `defaults.limits` now takes only `max_records` and `max_duration_ms`.
- **A viewport** is not a field of `environment`. It belongs to one kind of surface, so it is a field of that surface, in the schema its pack registers, and travels in the surface's payload. The kernel's `environment` holds `locale` and `timezone` and nothing else.
- **Whether a provider knows a setting's name** is not the compiler's to check: it holds no list of providers. The provider's own check refuses it when the version is published.

**New words in `a2m-core`.** Closed vocabularies `SignInMode`, `DriftRisk`, `CaptureLevel`, `ApproveWrites`, `WhitespaceRule`, `DecimalMark`, each held to its document by the documents test. The checked name `AuthProviderName` and the reference `CredentialRef`, which `a2m-auth` now uses in place of its own. One lint code, `kernel.session_check_invalid`.

**The plan format stays 1.** A plan written this morning does not decode now. No release has carried a plan, so there is no stored plan to read. From the first release on, a change like this raises the number.

**One home for the source definition.** `a2m-sources` depends on `a2m-files` and uses its `SourceSpec`, `TemplateVersion`, `Fingerprint`, `FingerprintPart`, `PartKind`, `RowField`, `Locate`, `HiddenRule`, `ReaderOptions`, and `OPERATION_READER`; its copies are removed. `differences` is a function of `a2m-sources`, since it makes that crate's problems. A test in `a2m-sources` compiles the `example` target and reads its fixture sheet through the plan.

## Consequences

- `a2m-actions` builds the keeper's `AuthEntry` from an `AuthPlan` field by field: `provider`; each setting by name; each credential by role; `idle_ms` and `absolute_ms` as durations; `attempt_limit`; `movable`. It runs `ProbePlan::Operation` through the engine and hands `ProbePlan::Entry` to the pack of the surface.
- `a2m-telemetry` has a `CaptureLevel` and `a2m-policy` an `ApproveWrites` of their own, with the same names. Each should become `a2m-core`'s. Policy's list lacks `caller_confirms`.
- The example in section 205 and the file format reference are changed to match.
- A target whose `timezone` is a named zone no longer compiles. Before, it was refused only where a date-and-time type took the fixed zone.

## Alternatives considered

- **The whole auth entry as one opaque payload.** The keeper would decode a shape the kernel owns, and a credential's value could hide in it.
- **Defaults in the plan, applied by the engine.** The engine would resolve at call time, which section 206 forbids.
- **A probe as free text.** An unknown name would be found at the first check of a session, not when the target compiles.
- **The formatting facts as flat optional fields.** A text type could then state a decimal mark. An enum by kind of value cannot.
- **Raising the plan format to 2.** It would say a stored plan of format 1 exists.

## Revisit if

A pack's auth provider needs a setting that is not text; the engine's design gives a decision point more than a name and a question; or a probe needs inputs that differ by environment.

## Spec sections

- [205. Target files](../spec/2-kernel/205-target-files.md)
- [206. The compiler and the plan](../spec/2-kernel/206-the-compiler-and-the-compiled-target.md)
- [215. Sessions and the session keeper](../spec/2-kernel/215-sessions-and-the-session-keeper.md)
- [File formats](../reference/file-formats.md)
- [`a2m-core`](../architecture/crates/a2m-core.md), [`a2m-files`](../architecture/crates/a2m-files.md), [`a2m-sources`](../architecture/crates/a2m-sources.md)
