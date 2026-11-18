# 0249. The HTTP pack reads in bounded work and sends nothing that may write outside a live run

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** the reader of XML answers, the JSON reader, the answer forms, and the driver of [`a2m-pack-http`](../architecture/crates/a2m-pack-http.md); its manifest (one failure code, and the lint `http.effect_method` made an error); a new endpoint key `read_asserted`; sentences of spec section [303](../spec/3-packs/303-the-http-pack.md); new routes of the test application under `/views/hostile/` and `/views/search/`. Answers findings S1, S2, and M1 to M5 of the safety audit of 2026-11-18 of decision [0217](0217-the-http-pack-reads-answers-that-are-not-json.md) and the pack's driver.

## What was verified

Every finding was checked against the code before it was changed. Each held. One was larger than reported: the JSON reader, which the audit did not read, compared each name of an object with every name before it, so an object of a few hundred thousand names under the ceiling of an answer took time that grows with the square of its names. It was fixed with S1, as S1's third part asks of every form.

| Finding | Verified | Fix | Test that fails before it |
| --- | --- | --- | --- |
| S1. A namespace address copied for every element and attribute, on an async thread | Yes: `Resolved` held an owned copy of the address for each open element and each matched attribute, and `validate` read the body synchronously inside the driver's async `act` | 1. An open element is held as the position of the declared name it equals, or none; names are compared with the declared ones by borrowing the reader's text, never copied. 2. Bounds, each refused by name as `unknown_variant`, `response_too_large`: a namespace's address 2,048 bytes; a name of an element or an attribute, with its prefix, 256 bytes; an attribute's value, as written, 65,536 bytes; 64 namespaces declared at once (the reader's own limit, set by the pack and named). 3. Bounded in work: every read of an answer, in every form, runs on a blocking thread (`tokio::task::spawn_blocking`, as the data pack runs its statements). The step waits for it until the step's deadline; at the deadline it sets a flag the readers look at, and goes on without waiting: the step is `transient`, `timeout`, or for a write's answer `possible_commit`. The XML reader also counts its work, one unit for each event, attribute, byte of text taken, and comparison with a declared field, against a budget of 4 units for each byte of the body for each declared field and path step, plus 65,536; past it the read stops. The JSON reader keeps the names of an object in a set and looks at the flag every 4,096 values | Unit tests in `xml.rs`: the audit's document (refused at its first element, in at most 16 units); the same at the bound against a declared address of the same length (read, under a fixed budget of 2,000,000 units); nesting at the bound; a long element name, attribute name, attribute value, and 65 namespaces (each refused in at most 100 units); 300,000 attributes on a matched element; the budget and the flag. Each asserts the units spent under a fixed budget, and a wall time under 5 seconds. A unit test in `json.rs`: 200,000 names. A unit test in `driver.rs`: on a runtime of one thread, work that never ends by itself is left at the deadline and stops when told. An end-to-end test reads the audit's document from the stand |
| S2. No read-only guard of the driver's own outside `Live` | Yes: the only guard was `writes && !commits`, and the driver kept no posture | The rule below. The driver keeps the posture and whether the run's environment is of kind `test` from `prepare`, as the command pack does | `tests/answers.rs`: a search behind `POST`, in each posture and environment, asking the stand's log of requests whether it arrived; a write to the declared test record in each posture, asking the stand what it stored |
| M1. The first of two matching attributes won | Yes | Every attribute of a matched element is looked at; two that resolve to the declared name, through two prefixes of one namespace, are `drift`. One name written twice is refused by the reader | `two_attributes_that_match_one_path_are_a_stop` |
| M2. Disagreements about the character set taken silently | Yes, all four | Refused as `drift`: a second `Content-Type` header; a second `charset` in one; a byte-order mark before a text declared `us-ascii` or `iso-8859-1`, or UTF-8's mark before an XML document that declares US-ASCII; an XML declaration whose encoding is not the one the header names; a header, or a declaration, that names US-ASCII over a body that is not ASCII; a declaration not at the start. A JSON endpoint that declares its content types now also has its charset checked: UTF-8, or US-ASCII over an ASCII body | `a_disagreement_about_the_character_set_is_a_stop`, `the_document_and_its_header_agree_about_the_character_set_or_it_is_refused` |
| M3. A `bytes` answer neither checked nor keyed | Yes | Its body passes the redactor's check before its length and digest are given: a body that holds a registered secret is `unknown_variant`, `answer_holds_secret`, a new code, and nothing of it is returned. The body is read as UTF-8 with each invalid sequence replaced, which keeps every valid sequence whole, so a secret between bytes that are not text is found. **Not keyed**: the purposes in `a2m-core` are a blob's name, a restricted value in a trace or a diff, a record's key, and a caller's key; none is "a body given to a caller", and no keyed digest, nor the workspace, reaches a driver. The digest is made in one function, `forms::body_digest` | `a_body_that_holds_a_registered_secret_gives_no_length_and_no_digest` (unit), `a_body_that_holds_a_registered_secret_is_not_confirmable_through_its_digest` (against the stand) |
| M4. CDATA and references outside the root element | Yes | Refused as `drift` before the root element opens and after it closes | `a_cdata_section_or_a_reference_outside_the_root_element_is_refused` |
| M5. Control characters in values | Yes, for XML references and raw bytes, text bodies, pattern groups, and JSON escapes | One rule for every form, in one place, where the `extract` step takes a value: text that holds any of Unicode's control characters but the tab, the line feed, and the carriage return is `drift`, `response_schema_mismatch`, naming the field and no character of it. The text of a declared refusal that holds one is left out of the failure. **Why**: a tab and a line break are ordinary in text. Every other control character has no meaning a caller's text needs and several a terminal or a log gives them, such as the escape that starts a colour or a cursor move; a value is data and must not act on what shows it. Unicode's formatting characters, such as the marks that turn the direction of text, are kept: they are ordinary in text written right to left | `a_tab_and_a_line_break_are_kept_and_every_other_control_character_is_refused` (unit), `a_value_is_bounded_data_and_a_control_character_in_any_form_stops` (against the stand, XML and text) |

## The read-only rule

Outside a live run, the driver sends a request that **may change the system**, by its method (anything HTTP does not mark safe: `POST`, `PUT`, `PATCH`, `DELETE`) or by its endpoint's `effect: write`, only:

- as the **commit point of a write to a declared test record**, in the context the engine prepared for it (`Sandbox { test_records: true }`, a step marked `commits`); or
- for an endpoint marked `read` that says **`read_asserted: true`**, as any step, against an environment of kind `test`, in the sandbox.

A discovery sends none. Refused, the request is `policy`, `read_only_posture`, the provider contract's code, and nothing is sent. A request by a safe method of an endpoint marked `read` is sent in every posture, as before.

At compile time the lint `http.effect_method` is now an **error**: an endpoint marked `read` whose method HTTP does not mark safe compiles only with `read_asserted: true`, which is the developer's word that the request changes nothing, after the system's owner confirms it. `read_asserted` on any other endpoint is refused too. A draft never writes it: to the drafter such an entry is still a write, as before.

**Why both, and why the environment.** Many services put a search behind `POST`, so refusing every unsafe method marked `read` would make them unusable. A warning, as before, let a careless or steered draft mark a writing request `read`, and then a shadow, a try, or a discovery sent it. The assertion makes the developer say it, once, where it is reviewed. The driver cannot tell a try from a shadow: the engine prepares both as `Sandbox { test_records: false }`, and changing the provider contract is not this package's. So the place decides: an environment of kind `test` is a copy the target declares safe to write to, and only there does a request nobody has proven is a read go out outside a live run. In production, such a read is used live only, once published; it is tried against a `test` environment. This is the command pack's rule for commands in the sandbox.

**Alternatives weighed.** (a) Refuse every unsafe method outside `Live` but a test record's commit: a search behind `POST` could then never be tried or shadowed anywhere, and a binding over one could earn no evidence. (b) Let an asserted read be sent in every sandbox posture: a shadow in production would send, once per live call, a `POST` that a person never tried on purpose, which is what the audit found. (c) A new posture for a shadow, so that a try may send an asserted read in production and a shadow may not: the better design, and a change to the public provider contract and to the engine; recorded below as owed.

## Bounds and their defaults

| Bound | Default | Set by |
| --- | --- | --- |
| A namespace's address | 2,048 bytes | Fixed |
| A name of an element or an attribute, with its prefix | 256 bytes | Fixed |
| An attribute's value, as written | 65,536 bytes | Fixed |
| Namespaces declared at once | 64 | Fixed |
| The work of one XML read | 4 units for each byte for each declared field and step, and 65,536 | Fixed |
| Depth, elements | 64, 100,000 | The endpoint's `max_depth`, `max_elements`, as before |
| Size | 8,000,000 bytes | The pack's setting, or the endpoint's `max_response_bytes`, as before |

The fixed bounds are far above what an ordinary document holds; an endpoint that needs more of one is a change to this record (kind: Not yet).

## What this does not cover

- **A pattern's match cannot be stopped midway.** A text answer's pattern runs in the `regex` crate's linear matcher, in time that grows with the text and the compiled pattern (bounded at 1 MiB), on the blocking thread. At the deadline the step returns; the thread ends when the match does. The flag is looked at before and after the redactor and before the match.
- **A read binding sends no body** (kind: Not yet). A search behind `POST` takes its inputs in the query.
- **Rendering at the doors** was not checked. At the pack's boundary a value is a plain text value, passed through the redactor, no longer than the answer's ceiling, and with no refused control character; that the operation marks free text from the system as `untrusted_text` is the plan's, and is shown by the test. How the command line, the REST door, and the MCP door print a value is not this pack's.
- **The discoverer's probe** returns the text of a description to the discoverer, not to a caller; its control characters are not refused.

## Owed elsewhere

- A purpose for a keyed digest of a body handed to a caller, in `a2m-core`, and a keyed digest scoped to the workspace in the provider contract's `PrepareContext`, so that `forms::body_digest` can be keyed. Until then the digest is the plain SHA-256 of a body that holds no registered secret.
- A posture, or a mark on the prepare context, that tells a shadow from a try, so that an asserted read can be tried in production and never shadowed there; or an engine that does not shadow a binding whose endpoint the file kind calls a write (`FileKind::writes` is true for an unsafe method marked `read`).
- The technical reference's table of failure codes gains `answer_holds_secret` (`unknown_variant`), from the pack's manifest through `tools/failure_codes.py`.
