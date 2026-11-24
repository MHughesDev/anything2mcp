# 0315. Every declared output is in every result, on every path

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Completes [decision 0305](0305-an-output-that-may-be-absent-is-declared-so-in-the-plan-and-the-output-schema.md) for the engine and for the packs other than HTTP. Carried on by [decision 0342](0342-a-documents-cell-may-be-empty-unless-it-is-a-key-or-declared-required.md), which supersedes the sentence of point 6 on a document's cells: a cell that is not a key and not declared `required: true` may be absent.

**Date:** 2026-11-23

**Changes:** `outputs_of` and `output_value` in `a2m-engine`, the output schema of a list in `a2m-files`, `at_path` in `a2m-pack-http`, the statements file and the lowering of `a2m-pack-data`, the lowerings of `a2m-pack-file`, `a2m-pack-stream`, and `a2m-pack-mcp`; roadmap package FT7b.

## Context

Decision 0305 made a tool's output schema list every output under `required`, and admit `null` only for an output that may be absent. A review of the build found that only the HTTP pack held a result to that. The engine let a `null` through in any output, and on every write path it left out an output that its steps had not observed, so a write's result could lack a key its schema requires. A list output that may be absent admitted `null` for the whole list but not for an item, and the HTTP pack counted a required value of the type `any`, which admits `null`, as certain.

## Decision

1. **The engine holds every result to its schema, whichever pack gave it.** An output that was observed as `null`, or a list output with a `null` item, stops the run with `unknown_variant`, `output_unexpected`, unless the output may be absent.
2. **An output that was not observed is `null` if it may be absent, and stops the run if it may not**, on every path: a read, a write read back, a record found unchanged, a record settled on resume or by reconcile, and one that ended on the system's acknowledgement. The key of each declared output is in every result, as decision 0305 point 2 says.
3. **The schema's `required` keeps listing every output.** The other choice, listing only what every result holds, would make the schema of a write promise nothing a client can check, and would let two results of one tool have different keys.
4. **A resumed record whose output the ledger holds masked stops**, with the same code. The mask is not the system's value, and `null` would say the system holds none; the engine's rule that a masked value is never used as an output stands.
5. **A list output that may be absent admits `null` for each item as well as for the whole list.** A value taken through `*` may be absent one item at a time.
6. **Packs flag what they know.** The HTTP pack counts a value of the type `any` as not certain. The data pack reads `nullable: true` on a field of a statements file, drafts it for every column the store says takes a null and that is not part of the key, and reports those fields as outputs that may be absent. The file pack reports the size, the change time, and the digest of a folder's entry as outputs that may be absent, since a folder has no size or digest and a machine may not say when an entry changed. A pack whose values could be `null` before this decision, and which cannot tell from the target's files which are always there, flags every output but those it knows are, so that no read that returned a `null` before now stops. The stream pack holds certain only a body field the channel's schema requires, on a verb that returns the message, and a list's count, `more`, and coverage; a message's other fields, its key, its place, and what a wait gives may be absent. The MCP pack holds certain only the record's key, which its driver requires, and the text of a tool whose result is text: the server's output schema is pinned only by its digest, and the target's files do not hold it, so every other member may be absent. The certain names travel in the payload of the pack's steps. A document's cells in the file pack are left as values that may not be absent: an empty one stops the run, and a person who knows the value may be missing declares `may_be_absent: true` (decision 0305 point 5).

## Alternatives

| Alternative | Why not |
| --- | --- |
| The schema's `required` lists only what every result holds | The compiler cannot tell which outputs a write's read-back observes, so a write's schema would require nothing; the caller could not tell a missing output from one the tool lacks |
| A write path leaves an unobserved output out, as before | The result does not fit the schema the client was given, and a client that validates it refuses a write that landed |
| Each pack checks for `null` in its own driver, as the HTTP pack does | Seven packs would each owe the same check, and a pack that forgot it would hand a caller a value its schema refuses; the engine is the one place every result passes |
| The data pack flags every column that is not part of the key | The store says which columns take a null; flagging the rest too would make the schema promise nothing for them |

## Consequences

A write whose read-back does not observe a declared output that may not be absent now stops after the write landed: the ledger holds the record as verified, and the failure names the output. This is a fault of the target's files, and the next step says to compare the operation's outputs with what the system returns. The command pack returns `null` for a member its output does not require, and is changed by its own package. The desktop and device packs return none.

## Proof

`a_null_stops_the_run_in_an_output_that_may_not_be_absent_whichever_pack_gave_it` and the list-item test in `crates/a2m-engine/tests/contract.rs`; `a_write_whose_read_back_does_not_observe_an_output_gives_null_or_stops` and `a_record_found_unchanged_gives_every_output_and_on_resume_is_read_again` in `crates/a2m-engine/tests/write.rs`; `a_list_output_that_may_be_absent_admits_null_items_in_its_schema` and `an_output_from_an_untyped_required_property_may_be_absent` in `crates/a2m-pack-http/tests/compile.rs`; `a_nullable_column_is_an_output_that_may_be_absent_and_comes_back_null` and `a_column_that_takes_a_null_is_drafted_nullable_and_its_outputs_may_be_absent` in the data pack; the folder list in `crates/a2m-pack-file/tests/entries.rs`; `an_output_is_certain_only_where_the_channels_schema_or_the_verb_makes_it_so` in `crates/a2m-pack-stream/tests/compile.rs`; `an_output_is_certain_only_where_the_mapping_makes_it_so` in `crates/a2m-pack-mcp/tests/pack.rs`.
