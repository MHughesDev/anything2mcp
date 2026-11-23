# Field test results

What the product showed when it was run against existing software that the project runs itself ([decision 0293](../decisions/0293-software-we-run-ourselves-is-tested-against-during-the-build-and-may-be-written-to.md)). Each system is described by its kind and never named (rule 9). How a field test is run is in [decision 0295](../decisions/0295-a-field-test-is-run-from-a-recipe-kept-outside-the-repository.md). A finding stays here until a stand holds it as a case and CI passes with it. Since decision 0310 the runner runs the system and the product's binary in containers on a closed network (`--internal`, no published port); the field test 1 results below were got before that, with the system's port published on this machine.

## Field test 1: an HTTP service with a published description, reads only (2026-11-23)

**The system.** A self-hosted application for hosting source code, started from its maker's container image, pinned by digest. It keeps its state in one embedded database file. It publishes a description of several hundred operations in the Swagger 2.0 form. It was given one invented administrator, one repository, and three issues. Its database file was read directly to confirm the three issues before the product was started.

**What was run.** `init`, `target add` with the kind `http`, `scout run` with the description as a file, `lint`, `draft delete`, and `try` on three drafted reads. Nothing was written through the product, and nothing was published. Linux only. The recipe ran three times with the same result.

**What worked.** The target was registered. A description in the OpenAPI 3.0 form was read into 20 findings and 39 drafts. One drafted read, of the system's version, ran and returned the right value. Every failure had a class, a code, and a next step.

**Findings.**

| # | What happened | Why it matters | Where it belongs | State |
| --- | --- | --- | --- | --- |
| 1 | The system's own description was refused as `description_unreadable`, because it is Swagger 2.0 and the reader takes OpenAPI 3.0 and 3.1. Nothing was drafted | Many services still publish the older form. For them the scout drafts nothing | `a2m-pack-http`, the description reader | Closed on 2026-11-23: the reader takes the Swagger 2.0 form; the system's description is read as published |
| 2 | The scout's summary said only that the description's discoverer was not looked at. It did not give the code or the fix, which the failure does carry | A person reading the summary cannot tell what to do | `a2m-actions`, the scout report | Closed on 2026-11-23: the summary gives the code and the fix |
| 3 | With no description, the scout asked the surface's own address twice, got "not found" twice, and stopped with two findings | A service with no description gets no drafts. That is the case the capture of a person's use is for, which is not built | Roadmap, the capture packages | Open |
| 4 | Every endpoint drafted from a description handed in as a file left its `surface` as an open question, though the target declares one surface of that kind. No draft compiled. The summary said there were no open questions | A person must edit every drafted endpoint by hand before anything can be tried | `a2m-pack-http`, the drafter; `a2m-actions`, the count | Closed on 2026-11-23: an endpoint is tied to a surface when exactly one surface of the target has its origin and begins its path |
| 5 | Two drafted operations were given the same tool name. One draft that does not compile stops every other draft of the target from being tried | A large description is likely to hold one bad draft, and it blocks all the good ones | `a2m-pack-http`, naming; `a2m-files`, whether a trial needs the whole target to compile | Half closed on 2026-11-23: no two drafts share a tool name or a file. The other half is finding 9 |
| 6 | Lint suspected a credential in a field whose name contains the letters of a secret's name: the ordering number of pinned items | False alarms teach a person to ignore the rule | `a2m-files`, the credential lint | Closed on 2026-11-23: a path into an answer is a name, not a value |
| 7 | The two reads of an issue stopped as `drift`, `response_schema_mismatch`: the system answers `null` for an optional object, and its description does not say the object may be absent | Descriptions in the wild seldom mark this. Two of the three reads tried stopped on it. Unknown means stop (rule 7) is right; what is wrong is treating a described object's absence as unknown without asking. This needs a design decision | `a2m-pack-http`; a decision record | Closed on 2026-11-23 by [decision 0297](../decisions/0297-a-property-a-description-does-not-require-is-drafted-as-possibly-null.md); test application case 35 holds it. Both reads return the values the software's database holds |
| - | The engine refused to start under a deep folder with `socket_path_too_long` | A named stop, with a fix. Recorded because a project in a deep folder is ordinary | None; the runner takes a short home | Not a fault |

**After the first fixes, the same day.** The recipe was run again with findings 2 and 7 closed: all three reads return, and the two that had stopped return the titles the software's own database holds. The recipe has 21 steps, of which four are still marked `gap` (findings 1, 4, 5, 6).

**With the whole description, the same day.** Once the reader took the older form of description, the scout was handed the system's whole description, several hundred operations. Two more findings:

| # | What happened | Why it matters | Where it belongs | State |
| --- | --- | --- | --- | --- |
| 8 | The scout's result was 1.2 MB, over the cap of 256 KB on a result, so the command returned no report at all, only that it was cut | At the size of a real system the scout cannot show what it did | `a2m-actions`: the scout answers with its summary and counts, and the lists are read in pages | Closed on 2026-11-23: the answer carries counts and the first entries of each list, and `scout.report` reads a list in pages |
| 9 | 79 problems stood, most of them open questions in drafts of operations whose answers the pack cannot give a shape to. No read could be tried until every one was dealt with | A person who wants three tools is made to deal with three hundred drafts first | [Decision 0298](../decisions/0298-a-draft-with-an-open-question-waits-in-a-holding-folder.md) | Closed on 2026-11-23: of the system's whole description, 152 reads were drafted in place and 69 files were held; lint lists them and a trial of a held tool is refused by name |

**After the second fixes, the same day.** The recipe now hands the scout the description as the system publishes it, converts nothing, and edits nothing by hand. Findings 1, 4, and 6 are closed and finding 5 is half closed. An unfriendly review of the reader's change, before it was accepted, found seven faults that no test had shown, among them an endpoint tied to the wrong surface where one surface's address lies inside another's, names that could still be shared, and work the reader did without counting it against its bounds; all were fixed and tested before this run. Two things the review and the fix left are in the Build Order under FT2: a read with a required request body in the OpenAPI 3 form is still drafted as a read, and two operations with one `operationId` would share an endpoint file.

**With the holding folder built, the same day.** Two more findings stand between the whole description and a read that can be tried:

| # | What happened | Why it matters | Where it belongs | State |
| --- | --- | --- | --- | --- |
| 10 | One drafted read maps a field of an answer whose name is a secret's (a client secret) as an output. The compiler refuses a secret in a file, as it must, and that one draft fails the whole target | A system that returns a secret in an answer is common. The drafter must never offer that field to a caller (rule 4), and must not let it stop the other tools | A decision record; `a2m-pack-http` drafting | Open **Closed** by [decision 0302](../decisions/0302-a-field-named-as-a-secret-is-never-drafted-as-an-input-or-an-output.md): the system's whole description compiles, three runs alike |
| 11 | The published description declares a sign-in. Every drafted endpoint therefore takes the session's credential, and a read the system answers without one waits for a sign-in | The first field test has not signed in to anything. The next one must: a stored token, the session keeper, and a read through it | Roadmap packages 2.2 to 2.6, with a field test | Open Decided by [decision 0303](../decisions/0303-a-declared-sign-in-is-kept-and-a-read-that-waits-for-one-is-reported-as-waiting.md); open until roadmap FT9. With finding 10 closed, the reads of the whole description wait for a sign-in too |

**What this field test does not show.** A write, a sign-in, a publish, a second operating system, or any kind of system but a request service. Findings 1 and 4 were got past by hand, with a conversion of the description and an edit of each draft, so that the later steps could run; the recipe keeps both as steps.

## Field test 2: the same service, one verified write with its answer lost (2026-11-23)

**The system.** The application of field test 1, started the same way, on the runner's closed network ([decision 0310](../decisions/0310-the-field-harness-runs-in-a-closed-network-and-says-in-the-ledger-what-it-is.md)), with the fault proxy between it and the product. Every command of the product ran as a thin client of the field harness, which the runner built into a build folder of its own for each run and deleted after it.

**What was run.** The 23 steps of field test 1, then a write: a second target written by hand, with one surface, one declared test record (one invented issue), a sign-in by `http_basic` from a stored invented password, a read of an issue and a change of its title by `PATCH`, and an offline test of each against a fixture written by hand. `lint`, `secret set`, `test run`, `version publish` carried out by the runner's `person` step on the engine's own page, then `tool call` of the write with the proxy set to cut the answer to that `PATCH` once, the `person` step once with another hash and once with the change's own, and `run resume` twice. Linux only. Three runs, all 60 steps alike in each.

**What it showed.**

- The write rested at `needs_approval`, and nothing was sent to the system before the approval.
- A prompt answered with another hash passed nothing: the page said so, the run still waited, and the system's database still held the old title.
- With the change's short hash, the approval was recorded. The ledger's approval row and the audit log record its presence as `field_harness`, once.
- The first `run resume` sent the write once, lost its answer, and stopped as `possible_commit`, `no_confirmation`, not retry-safe. The second reconciled by reading the record back, found the new title, and completed without sending again. The proxy's record shows one `PATCH`.
- The system's own database holds the new title exactly once, and its own interface, asked directly, answers it.
- The invented password appears in no step's output, report, or harness log.

**Findings.** Got while the recipe was written, by hand; none is in the recipe.

| # | What happened | Why it matters | Where it belongs | State |
| --- | --- | --- | --- | --- |
| 12 | A password of one character was stored and registered with the redactor, which then replaced that letter everywhere in every answer | A short secret ruins every answer and log; no secret of one character is a real one | `a2m-vault` or the redactor in `a2m-telemetry`: a lower bound on a secret's length, or a refusal by name | Open |
| 13 | After that secret was replaced, the offline test of the write still stopped as `input_invalid`, a value it would write holding a registered secret, until the engine was restarted; the cause was not looked into | A replaced value stays a secret for the engine's life, and refuses writes that no longer hold one | The redactor and `secret.set` | Open |
| 14 | `secret set` with a plain name stores `vault://<name>`, which a target's `auth.yaml` refuses as `credential_of_another_target`; a name of the form `<target>/service/<name>` is taken, though the command's help says only letters, digits, and underscores | A person follows the help and cannot store a credential a target can use | `a2m-actions`, the help of `secret.set` | Open |
| 15 | A drafted write cannot be tried in the sandbox, so a first write needs an offline test with a fixture written by hand and a publish | Known: `draft.try` says a trial that writes needs the publish gates' test records, which are not built. The cost of the first write is a fixture by hand | Roadmap, the trial of a write on a declared test record | Open |

**What this field test does not show.** A write the system cannot read back, a connection cut before the request, a kill of the engine between intent and act, a sign-in typed on the engine's own page, or a second operating system.
