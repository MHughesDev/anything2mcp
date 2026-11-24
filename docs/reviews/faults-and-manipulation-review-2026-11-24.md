# Faults and manipulation: a review round, 2026-11-24

The round of package 3.10, by lens 5 (self-failure: what happens when each of the engine's own parts fails?), with the questions of lens 4 (the steered agent) asked of the same tests, as the package asked. It read [spec section 224](../spec/2-kernel/224-when-the-engine-itself-fails.md) and [section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md) in full, the designs of the kernel crates they name, and what the package's tests showed. It changed no product code; the code the package changed is in [decision 0399](../decisions/0399-the-faults-of-the-kernels-own-parts-as-proven-and-what-a-verified-record-keeps.md).

The evidence came first: each row of section 224's table that names a kernel crate, and each defence of section 502 that names a door or a reader, was made a test, and the product's test `crates/anything2mcp/tests/faults.rs` makes the faults of a write against the test application and asks it what it stored. Where a test showed the documents wrong, the finding is below.

## Findings, most important first

| # | Finding | Kind | Serious | Where it went |
| --- | --- | --- | --- | --- |
| 1 | A run killed after its record was verified and before it completed was never completed by a resume (package 3.7, seeds 8, 11, 34, 40 of 60): the outputs only its read-back gave were not kept, and the stop that followed was not recorded on a run already `stopped` | Gap, in the ledger and the engine | High: a run that wrote once could never be finished | Fixed in code; decision 0399, points 4 and 5 |
| 2 | Section 224 says the engine refuses a half-migrated store; the ledger applied every migration it had not recorded, over the objects such a migration had already made | Contradiction | Medium | Fixed in code and in section 224; decision 0399, point 1 |
| 3 | Section 224 says a new policy version that fails to load leaves the previous in force, and that with no valid version everything is denied; the code denies everything when the newest stored version cannot be read, although an older one can. Both readings are safe only one way: falling back may loosen | Ambiguity | Medium | Section 224 says which: a failed version is never stored; an unreadable newest one denies (decision 0399, point 2) |
| 4 | Section 224 says calls that need the key store wait, then fail; they fail at once | Contradiction | Low | Section 224 changed to what is built (decision 0399, point 3) |
| 5 | An engine started while the key store is unreachable stays `degraded` until it is started again; section 224 says recovery is automatic | Gap, in `a2m-actions` | Medium | Section 224 marks it not built; section 704; left for the action registry |
| 6 | A session that ends between a write and its read-back leaves the run `needs_login`, whose fix tells the caller to run the tool again; the write may have landed. The resume does reconcile first and writes nothing twice | Weakness, in `a2m-engine` | Medium: the words invite a second call that a person might make by hand | Left, with its reason, in decision 0399 and roadmap row 3.10 |
| 7 | A surface an edge blocks stops a write as `unknown_variant`, not `edge_blocked`: nothing in the product is a source of edge blocks for a run | Gap, in `a2m-actions` and the HTTP pack | Low: nothing is written, and a block during a read-back leaves the record in doubt | Left in decision 0399; section 704 |
| 8 | Section 502 did not say what becomes of an input that reads like an expression | Gap | Low | Section 502 now says it is a value, sent as text encoded for its position (shown by the product's test) |
| 9 | Section 224's "Proving it" asks each fault test to assert the health state; the product's fault tests assert what callers were told and what the stand holds, not the health state | Weakness | Low | Left for package 9.3, which builds every row as a test |

## What the steered-agent questions found

Nothing new that a steered agent could do. An input naming a path, an address, a header, an undeclared field, or an expression reached nothing at the stand, or reached it only as text; a header could not make or change a caller at the REST door; text from the system that reads as an instruction, as a protocol message, and as terminal control came back from the MCP door as one string value, with its control character escaped, in one answer, and changed neither the tools listed nor what ran. A name with `..` or a link was already refused by `a2m-files` (`the_loader_never_reads_outside_the_target_folder`, `a_draft_is_never_written_through_a_link`) and by `a2m-sources`, which is handed bytes, never a path (`a_part_named_from_another_stays_inside_the_package`).

## Returns

The round found most by building the tests first: findings 1, 2, 5, 6, and 7 were found by a test and by no reading. The next gain is more likely from building package 9.3's full suite, and from lens 13 (the other operating systems), since a Windows run found a fault in a store test this week that no reading had.
