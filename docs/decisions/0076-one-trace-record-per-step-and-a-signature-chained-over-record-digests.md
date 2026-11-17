# 0076. One trace record per step, and a signature chained over record digests

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** four open points of [spec section 217](../spec/2-kernel/217-traces-evidence-and-repair.md) and of the [`a2m-telemetry` design](../architecture/crates/a2m-telemetry.md): the unit of a record, how a restricted value enters the signature, the check on the way out, and the allowlist of headers. It also fixes what the signature covers and the first version of the trace schema. **Changes sentences of section 217.**

## Context

Roadmap package 1.2 builds the step trace and its signature. Section 217 named the parts the signature should cover and left the exact rule open. The engine, the store, and the publish gates are coded against the result. Hypothesis H-16 says the signature is identical across runs and operating systems.

## Decision

**The record**

- **One record per step,** where a step is one call of a driver's `act`. A step that is retried is two records. Each record names its pass (one turn of the engine loop), its part of the loop (`act`, `confirm`, `read_back`), and its position in that list in the plan. *Instead of:* one record per pass, which has one locator and one outcome where a pass has several.
- The binding choice, the queue time, and the prepare time are on the first record of a pass. Each record has the time its own step took. The act, confirm, and read-back times of a pass are the sums of their steps.
- What a driver did is two lists of named details that the pack fills: `means`, which must be the same in two runs that did the same thing, and `measured`, which may differ. The kernel knows no kind of system, so it cannot say which of a status, a size, or a rung belongs where. The pack's design says.
- The record key is never written. The record holds the workspace's keyed digest of the key's canonical bytes, under the purpose `record_key`, which is how the store names a key.

**The signature**

- Each record gives one SHA-256 digest over these, in this order, each written as a name and a value with their lengths: pass, part of the loop, position, step kind, the record key's keyed digest, the operation and the mode when the record starts a pass, the locator and its position, the `means`, then for every value its field name, whether it is volatile, its two normalized forms unless it is volatile, and whether they matched, and last the outcome with its failure class and code.
- **The signature chains the record digests.** It starts from a fixed digest, folds in each record's digest in order, and closes with the count. *Instead of:* one hash over all the bytes, which is what section 217 said. The chain needs constant memory, and each record's digest is written in its line as `sig`, so two traces that differ show where: at the first record whose `sig` differs.
- **Left out:** every timing; the run, the attempt, the workspace; the tool called and the plan's hash; the engine's version, the registry's digest, and the provider chosen; why a binding was chosen; the fingerprint score; the `measured` details; the raw values before normalization; artifact references; the capture level; and a volatile field's value.
- The plan's hash is left out for a reason beyond identity: a test in a target's files may expect a signature, and those files are compiled into the plan. A signature that covered the plan's hash could never be written into the plan.
- **A value that is not public enters as its keyed digest, at every capture level.** So the signature is the same at every level, and it never lets a guess be confirmed that the trace itself would not. *Instead of:* a plain digest, which a short value does not survive. A public value enters as its canonical bytes.
- **The signature is taken after redaction and cleaning, and before text is cut for size.** A session token that differs between two runs is masked before the signature sees it, so it does not make them differ. Two long texts that differ past the cut have two signatures.

**Keeping secrets out**

- Text is redacted, then cleaned of markup and hidden characters, then redacted again, because cleaning can join the halves of a secret that markup had split.
- **The check on the way out is `Redact::contains_secret`** ([decision 0069](0069-the-plan-is-canonical-json-hashed-whole-and-keyed-digests-are-per-workspace.md)). It runs over the finished bytes and over every text in them with the JSON escapes undone. A hit refuses the whole trace: `finish` returns no bytes, the signature, and a `redacted` limitation for the engine to record in the trace's place. As a failure it is `input`, `trace_holds_secret`.
- A label decides how a value is held: `public` as it is; `internal` as it is at `standard` and above and as a keyed digest at `minimal`; `restricted` only ever as a keyed digest.
- **The allowlist of headers** is one constant, `RECORDED_HEADERS`: twenty headers that describe content, caching, and negotiation. A header that is not on it is recorded as present with its value masked. Cookies, authorization, and headers that carry an address, such as `location` and `referer`, are not on it.

**The bytes**

- JSON Lines, schema 1: a header line, one line per step, an end line. Entries are written in a fixed order with no spaces; an entry with no value is left out. The end line carries the count, the signature, and the limitations, so a cut trace can be told from a broken one.
- A trace past its size limit is cut: later records are left out of the bytes and still enter the signature. This is a `truncated` limitation, not a failure.

## Consequences

- A signature compares within one workspace whenever a run has a record key or a value that is not public. A test that expects a signature is portable only if the test sandbox runs under a key that is fixed and public, which is safe there because fixtures are synthetic. The slice that builds the test runner decides this.
- The store must digest `RecordKey::canonical_bytes()` under `record_key` for a trace's `record` to match its rows.
- `TraceBuilder::new` takes a `KeyedDigest` beside the `Redact`, and `TraceIdentity` carries the workspace.
- The raw value of a step that has no normalized form does not enter the signature. The engine gives a normalized form for everything it compares.
- Section 217 is corrected where it said one hash over the bytes, and its four open points are closed.
- What this showed about H-16: five simulated runs with different identifiers, timings, scores, sizes, and provider digests have one signature, and that signature is written into the test as a constant, so the three operating systems in CI must each produce it. Real runs through a driver are still to come.

## Alternatives considered

Given beside each choice above.

## Revisit if

A pack cannot say which details are `means`; a real driver shows variation the signature should ignore; or traces grow past what holding one in memory until `finish` allows, which is when the `EvidenceSink` the design weighed becomes the answer.

## Spec sections

- [217-traces-evidence-and-repair.md](../spec/2-kernel/217-traces-evidence-and-repair.md)
- [403-testing-and-publish-gates.md](../spec/4-building-tools/403-testing-and-publish-gates.md)
- [The `a2m-telemetry` design](../architecture/crates/a2m-telemetry.md)
