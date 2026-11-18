# 0139. A write that was sent is never settled by an answer that is not the application's verdict

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the design of [the HTTP pack](../architecture/crates/a2m-pack-http.md), [spec section 303](../spec/3-packs/303-the-http-pack.md) (the driver's rules, optimistic locking, the failure table), one row of the [file formats](../reference/file-formats.md#endpointsyaml), and one fault of the test application. Builds on decisions [0096](0096-the-http-packs-endpoint-entry-steps-and-payload-as-building-them-settled-them.md) and [0097](0097-what-the-provider-contract-could-not-give-the-http-driver-and-what-the-pack-does-instead.md).

## Context

A safety review read the HTTP pack against its documents and the ten rules, and ran nothing. It gave eight findings. Each was checked against the code with a test that failed before the fix; all eight were confirmed. Working through them showed three more defects of the same kinds, listed last.

The ledger takes a failed record as settled, with nothing to reconcile, when the failure's class is `policy`, `input`, or `stale`. That is the fact the first finding rests on.

## Decision

1. **A write that was sent is settled only by the application's verdict** (finding 1, critical, confirmed). A write that the system made and answered with `303` to another origin was reported as `policy`, `surface_not_declared`, so the ledger marked it failed and not in doubt, and the test application held the write. After the commit step's request may have reached the system, the driver now returns a settled class only for an answer the endpoint declares as a conflict. A redirect to another origin, an answer too large to read, and a `validate` or `extract` step that cannot be carried out while a write's answer is held are all `possible_commit`, `no_confirmation`, sent unknown. The redirect is still never followed, and the address the system named is not quoted. A redirect within the surface is a status like any other: accepted when the endpoint declares it under `ok`, and otherwise `unknown_variant`, `unexpected_status`, which the ledger keeps in doubt. *Instead of:* a new class outside the settled set that also names the policy finding, which would need the engine and the ledger to learn it; and instead of turning a sign that the session ended into `possible_commit` too, which would hide it from the keeper. That sign stays `session_expired`, which the ledger does not take as settled.
2. **The version precondition is kept until a request may have reached the system, and a write that declares one is never sent without knowing the record's version** (finding 2, serious, confirmed). The stored version was removed before the request was sent, so the engine's retry of an attempt that could not connect went out with no version. Against the test application, which refuses an update with no version, the retry came back `rejected`; against a system that takes a write with no precondition as unconditional, it would overwrite another caller's change. What a pass knows of a record's version is now one of two things, kept by the endpoint that read the record and the record's key: the version, or that the record was looked up and is absent. Nothing known is a refusal, `stale`, `version_not_read`, not sent. The write's step names the endpoint whose read supplies the version, and the lowering refuses a binding whose `read_back` endpoint reads none. *Instead of:* keeping the version for the whole pass, which would send a used version again after a write that landed; and instead of keying by the record's key alone, as before, under which two operations of one run whose records share a key, such as an order and its delivery details, would take each other's version, and every step for no record shared one slot.
3. **Reading a description is bounded in depth, in steps, and in bytes, and past a bound the import is refused by name** (finding 3, serious, confirmed). Twenty schemas that each name the next one twice, 2,588 bytes of description, were expanded into 3,145,726 parts in five seconds; each schema added doubles that. The bounds are 32 levels, 20,000,000 steps, and 16,000,000 bytes of shapes, with each reference looked up once; the refusal is `input`, `description_too_large`, for the whole import. A schema that holds itself is found by the references that are open, at once, and leaves only its own body without a shape, as before. *Instead of:* a memo of converted schemas alone, which saves the work and not the size of what is written; and instead of leaving out the one body that is too large, which would read a part of a description as if it were all of it.
4. **Where a redirect leads is read against the address of the request** (finding 4, minor, confirmed). `//host/path` and `/\host/path` were not seen as leaving the surface, so a sign-in redirect written without a scheme was reported as an unknown status and the keeper was not told.
5. **A body that is not JSON fits no shape** (finding 5, minor, confirmed). Under `type: any` a page answered with `200` was read as nothing. It is now `drift`, `response_schema_mismatch`; only an empty body is read as nothing. *Instead of:* a way to declare a body that is not JSON, which no built step could read.
6. **The address is checked where it is sent, path and query included** (finding 6, minor, confirmed as a missing second check; no input reaches it). The driver sends only to an address that reads back as the surface's origin and exactly the path and query that were built, and whose built path starts at a `/`. This is stronger than comparing the start of the path: it also refuses an encoded dot segment, which the URL type folds away, and a separator or a byte that it would read differently. The check showed that a built path with no leading `/` could leave the surface's path, or with an address that has no path, its host; nothing builds one, and it is now refused.
7. **A description passes the redactor before it is parsed, whichever way it arrived** (finding 7, minor, confirmed). A description given as a file reached findings unredacted. The text is redacted whole, before anything is cut or formatted, and each finding once more as it is written.
8. **A held answer prints no header and no body** (finding 8, minor, confirmed as latent).

Found on the way, and fixed the same way:

- A header the connection sets could be named as an endpoint's idempotency header or its version header, so a token or a version would have been sent as `host`. The compiler now refuses it, and the driver refuses it again where the request is built.
- A version read with no record key, and a version of another record type with the same key, could be sent as a write's precondition. Point 2 closes both.
- A write was sent with no version when the pass had not read the record at all. Point 2 closes it.

## Consequences

- The pack has two more failure codes, `stale`/`version_not_read` and `input`/`description_too_large`, declared in its manifest.
- The payload of a `request` step has one more field, `version_from`. A plan compiled before it, with an endpoint that sends a version, is refused as `input`, `step_not_usable`, with the instruction to compile again.
- A write whose answer is over its endpoint's size limit is now in doubt and reconciled, where it was `unknown_variant`.
- The test application has a fault, "redirect after commit": the request is carried out, then answered `303` with a given `Location`.
- The YAML reader of `a2m-files` already refuses anchors and aliases, so a description cannot grow as it is parsed. The review had left that unread.
- Owed by other crates, which this record does not change: the engine takes a record as fresh by time alone, and should read it again when the pass that read it was replaced, or the write is refused as `stale`, `version_not_read`; and a test in the ledger should hold that `session_expired` is never a settled class, since the driver relies on it for a write answered by a sign-in redirect.

## Revisit if

- A system needs a write that creates a record to carry a precondition that the record does not exist. None is sent today.
- A real description is refused by the bounds. The figures are judgment; none was measured against a large published description.
- The ledger's list of settled classes changes.
