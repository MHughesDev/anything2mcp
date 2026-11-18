# 0270. The HTTP pack sends an asserted read only from a published version, and bounds what it leaves running

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** the driver, the answer forms, the lint `http.effect_method`, the lowering, and the manifest of [`a2m-pack-http`](../architecture/crates/a2m-pack-http.md) (two failure codes); the credential sink and the manifest of [`a2m-pack-mcp`](../architecture/crates/a2m-pack-mcp.md) (one failure code); sentences of spec section [303](../spec/3-packs/303-the-http-pack.md) and of the [file formats reference](../reference/file-formats.md). Supersedes, of [decision 0249](0249-the-http-pack-reads-in-bounded-work-and-sends-nothing-that-may-write-outside-a-live-run.md), the place where an asserted read may be sent outside a live run, and its sentence that a pattern's match is bounded only by the step's time. Answers findings C1 to C4 of the second reading of decision 0249 (audit 6), and the HTTP and MCP half of its finding A1, whose web half [decision 0254](0254-a-person-signs-in-in-a-window-of-the-engines-own-browser-and-the-web-pack-stops-on-every-declared-sign-of-expiry.md) carried out.

## What was verified

Each finding was checked against the code before it was changed. Each test below failed before its fix: the end-to-end and compile tests against the code as it was, the unit tests with the one line of each fix taken out.

| Finding | Held | Fix | Test |
| --- | --- | --- | --- |
| C1. An asserted read sent outside a live run on two facts a draft writes | Yes. The guard took the environment's kind from the plan it was handed, which in a try is the drafts' | Below: only a published version's plan counts | `a_request_that_may_write_is_sent_only_where_the_posture_lets_it`: a try under drafts that mark the environment `test` sends nothing; the stand's log of requests is empty |
| C2. A method override passes as its outer method | Yes, for literal headers and query entries, and for a query entry a binding computes, which the audit did not name. A body's `_method` changes nothing: only a method HTTP does not mark safe carries a body | Below | `a_method_override_is_judged_as_the_method_it_names` (against the stand), `a_request_is_judged_by_its_method_and_every_override_in_it` (unit), `a_read_whose_method_override_names_a_method_that_may_write_is_refused` (compile) |
| C3. Reads left running are not bounded, and a pattern cannot be stopped | Yes, and wider: a read whose step is dropped, as a shadow's is at its budget, was never told to stop. Now its flag is set and it is counted when its step stops waiting | Below | `reads_left_past_their_deadline_are_bounded_and_the_next_is_refused_busy`, `a_read_whose_waiting_is_dropped_is_counted_until_it_ends`, `a_text_longer_than_a_pattern_is_matched_over_is_refused_before_the_match`, `a_pattern_longer_than_its_bound_is_refused` |
| C4. A `bytes` body checked as UTF-8 only | In part. Base64, hexadecimal, and percent-encoding are forms the engine's redactor already finds in text, and they are ASCII, so the UTF-8 reading found them: that half did not hold, and `a_body_that_holds_a_registered_secret_in_a_form_the_redactor_knows_gives_nothing` passed before and after. UTF-16 held | The body is also read as UTF-16, little and big endian, at both alignments, and each reading goes to the redactor | `a_body_that_holds_a_registered_secret_in_utf_16_gives_nothing` |
| A1, the sinks. Any header a provider places is sent | Yes, in both packs | A header whose name begins `a2m-`, in any letter case, is not kept; the pass is refused at prepare, `policy`/`product_header_refused`, naming the header and no value | `a_header_of_the_products_own_is_never_sent`, in `tests/session.rs` of the HTTP pack and `tests/driver.rs` of the MCP pack, asking the service or the stand what it heard |

## 1. An asserted read outside a live run is a published version's

Nothing in the prepare context says whether the plan is a published version's, and the provider contract is not this package's. One fact a driver holds does tell: a draft is always compiled as the draft version, `0.0.0`, and the compiler refuses a draft under any other (decision 0161); a published version is compiled under the version it is published as. So the driver counts an environment's kind `test` for an asserted read only when the plan's version is not `0.0.0`, and then both the kind and the address come from that published plan, which a person published. A version published as `0.0.0` is taken for a draft: the safe side.

Outside a live run an asserted read is therefore sent only by a try or a shadow of a published version, in an environment of kind `test` in it. A try under the drafts, which is how a new search behind `POST` is first tried, is refused with `policy`/`read_only_posture`: such a read is tried after a publish, or live.

**Alternatives weighed.** (a) Honour `read_asserted` in a live run only: safe, and a published search behind `POST` could then never be tried against a test copy or earn shadow evidence there. (b) A mark on the prepare context: the better signal, and a change to a public contract that another agent owns; owed below. (c) Keep the drafts' word, as decision 0249 did: the finding.

## 2. A method override is judged as the method it names

The headers `X-HTTP-Method-Override`, `X-HTTP-Method`, and `X-Method-Override`, and the query entry `_method`, compared without regard to letter case, are overrides; a query name is read percent-decoded, as a server reads it. A request may change the system by its method when its own method is not safe, or when an override in it names a method that is not safe, or a word that is no method (rule 7). An override never makes a request safer than its own method: whether a system honours one cannot be known, so the stricter of the two counts. `read_asserted` speaks of the endpoint's own method only, and does not let through a request whose override may change the system.

At compile time the lint `http.effect_method` refuses an endpoint marked `read` whose literal override names such a method, `read_asserted` or not; the lowering refuses, under the same code, a binding that sets `_method` for an endpoint marked `read`; and the file kind's `writes` answers true for such an entry, so a drafter never makes one.

## 3. What a read leaves running is bounded

| Bound | Value | Where |
| --- | --- | --- |
| Reads still running after their step stopped waiting, for one surface | 4 | Before the request is sent, and again before its answer is read |
| The same, for the pack | 16 | The same |
| A text answer's pattern, as written | 4,096 bytes | At compile time, and again before it is compiled at run time |
| A pattern, compiled | 256 KiB (1 MiB before) | The `regex` crate's own limit |
| The text a pattern is matched against | 1,048,576 bytes | Before the match |

A read is counted from the moment its step stops waiting for it, at its deadline or because the step itself was dropped, until its blocking thread ends; the count is the pack's own, in the driver. Past either ceiling the request is not sent: `busy`/`reads_still_running`. If the count rose between a request and the reading of its answer, the answer is not read: for a read, `busy`; for a write, `possible_commit`. A surface is its name at its origin.

**Still unbounded:** a pattern's compile and match, once begun, run to their end; their time grows with the text and the compiled pattern, both now bounded. Sixteen such reads may each hold a blocking thread and an answer of up to its ceiling, 8,000,000 bytes by default. The blocking pool is the engine's, shared with every pack, and its size is not this pack's to set. A pattern is still compiled for each read, on the blocking thread.

**Alternatives weighed.** A semaphore on every read in flight, as the audit proposed: it bounds reads that end in time too, which the engine's own limits on runs already bound, and a permit released at the thread's end needs the same count. Compiling the pattern once when the plan is loaded: it needs a cache keyed by plan in the driver, and the bound on the pattern makes the compile small; not done (kind: Not yet).

## 4. What a `bytes` body is checked in

The UTF-8 reading, with each invalid sequence replaced, and four UTF-16 readings, each with each unpaired surrogate replaced, all go to the redactor, which looks for every form it knows. Not covered: a secret compressed, encrypted, or written in any other encoding of text, such as UTF-32 or a code page; and a secret that is not ASCII, written in Latin-1. The keyed digest that decision 0249 owes would make the length and the digest unconfirmable whatever the body holds.

## 5. The product's own headers

A header name that begins `a2m-` is a carrier between the product's parts (the web pack's providers place the browser's state under one, named a2m-browser-state). The HTTP sink and the MCP sink each refuse one by name and place nothing; the pass is refused before any connection. Not done: the dedicated sink method the audit named as the cleaner fix, a change to the public `CredentialSink` contract.

## Owed elsewhere

- **`a2m-provider`:** `PrepareContext` gains `pub published: bool`, true only when the engine runs a published version's plan, which the engine takes from the job; the HTTP driver's `published(plan)` then reads it in place of the version.
- **`a2m-actions`, `src/control.rs`** (the test records the policy engine is told of, about lines 425-433): the same trust in a drafts' environment kind. The exact change: the filter that keeps an environment of kind `Test` also requires `!drafted`, so that it reads `.filter(|environment| !drafted && environment.kind == EnvironmentKind::Test)`, with `drafted` computed as the method that leases a session computes it (about line 270 of the same file).
- **`a2m-actions`, `version.publish`:** refuse the version `0.0.0`, the draft version, so that a plan's version always tells a draft from a published version.
- The technical reference's table of failure codes gains `product_header_refused` (`policy`) of both packs and `reads_still_running` (`busy`) of the HTTP pack, through `tools/failure_codes.py`.
