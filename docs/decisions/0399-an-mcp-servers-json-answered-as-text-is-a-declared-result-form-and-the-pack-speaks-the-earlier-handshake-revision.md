# 0399. An MCP server's JSON answered as text is a declared result form, and the pack speaks the earlier handshake revision

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md), from field test 6. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** the mapping file kind and the `mcp` driver of [`a2m-pack-mcp`](../architecture/crates/a2m-pack-mcp.md) (a result form `json_text`; the lint `mcp.write_no_readback`; the lowering of a read; the drafter's reasons; the revision `2025-06-18`), the replay of the provider contract ([`a2m-provider`](../architecture/crates/a2m-provider.md), `answer_name`), [spec section 304](../spec/3-packs/304-the-mcp-pack.md), the [test stand](../../targets/example-mcp/README.md), and findings 35 to 41 of [field test 6](../reviews/field-tests.md).

## Context

Field test 6 put a widely used MCP server over databases behind the pack ([results](../reviews/field-tests.md)). Reads worked; no write could be published. Seven findings, of which five had one cause each in the pack or the contract's replay:

- The server states no output schema and answers with JSON as text, one content item a row. The pack knew two result forms, `structured` and `text`. Under `text` an agent got one string holding every row, a read of a key the server does not hold gave an empty text and completed, and a write read back through such a read had no field to compare (findings 37 and 39).
- A read's binding with no `args`, over a tool whose mapping names `key_args`, compiled and passed its offline test, and its live call sent no key. The driver fills `key_args` from the record's key, and a read belongs to no commit point, so the engine gives it none (finding 38).
- The save's step refers to `surface/tool`. The replay matched an answer by the entry's last part, and named the step it was sent under the whole entry; a fixture's file carries the last part. So the request a fixture recorded was never the one compared, and no MCP write could pass the publish gate (finding 40).
- The read rule drafted nothing for three tools and said nothing; the report showed "the rule gave no reason" (finding 41).
- An earlier release of the same server speaks only the revision 2025-06-18, which the pack did not speak.

## Decision

1. **`result: json_text` is a declared form of a tool's answer: its text items are JSON.** An item whose text is empty holds nothing. With `records` named, every item is a row, and an item that is a JSON list gives each element as a row: the rows are listed under that name, so no item at all is an empty list, and a lookup or a read by one record of an empty list is absent. Without `records` the answer is the one item that is an object. Text that is not JSON, or several objects with no `records`, is `drift`/`response_schema_mismatch`, as a structured result that does not fit. The members are then passed on exactly as a structured result's are: only those the mapping lists. Nothing guesses the form: the mapping states it, as the command pack's output forms are stated, and a tool mapped `text` is read as before.
2. **A write is read back only through a read tool whose answer has fields.** The lint `mcp.write_no_readback` now refuses a write whose `read_back` tool is mapped `result: text`, at the mapping, instead of the offline test finding it as `unknown_variant`/`field_not_read_back`. Its fix names `json_text`.
3. **A read's step states the key arguments its mapping names.** Where the binding states none of its own, each argument under `key_args` is filled from the operation's key inputs, in order, as an argument of the step. So lint, the offline test, and the live call send the same thing. A write's steps are unchanged: their record carries its key.
4. **The name of a step in a fixture is the last part of its entry.** `answer_name` in the provider's replay gives `orders_update` for `orders/orders_update`: the name the engine is said to have sent, the name a recorded answer is written under, and the name a refusal tells a person to write `<name>.request.json` under. `answers_to` still takes either spelling. Two surfaces of one target with tools of one name share it, as their answers already did.
5. **The read rule says why it drafts nothing,** in the pack's own sentences: no target yet, a name that is not plain, a tool the server does not mark read-only, one string the server may run, a required input it cannot read or that is named as a secret's, and an output schema with no plain property. It also counts what it left out as a secret's.
6. **The held question of finding 35 is right, and the read follows its answer.** A read-only tool whose whole input is one string may be a read by key or a query the server runs; the definition cannot tell the two apart, and the read-only mark is a claim. So the question stays. What changes: when the target's mapping, outside the holding folder, already says `free_text: false` for that tool at the digest the server lists, the read rule drafts the read, so a person who answers and runs the scout again gets it without writing it by hand.
7. **The pack speaks 2025-06-18 as a client, through the same handshake.** A surface states `revision: "2025-06-18"`. What the pack uses is the same in it and in 2025-11-25: `initialize` and `notifications/initialized`, the `MCP-Protocol-Version` header, `tools/list` with a cursor, `tools/call`, output schemas, and structured content all came in or before 2025-06-18. The changes since that matter to a client are two. A server's error for a call's bad input may arrive as the protocol error -32602, which the pack stops on as a changed tool, answered and never tried again. No JSON Schema dialect is the default; the pack reads only the keywords it checks. Icons and the other additions of 2025-11-25 are not in a tool's pinned definition. The facts are from the two revisions' changelogs in the specification's own repository, `docs/specification/2025-11-25/changelog.mdx` and `docs/specification/2025-06-18/changelog.mdx`, read on 2026-11-24; the protocol's web site was not reachable from this session. Revisions older than 2025-06-18 are not spoken.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Read every `text` answer as rows when every item parses as JSON | A guess from the answer's shape: the same tool may answer prose another day, and an operation's outputs could not be typed from it. The form is the mapping's statement |
| Draft `json_text` for a tool with no output schema | The drafter sees the definition, not an answer. It keeps `result: text` and the person states the form |
| Fill a read's key in the driver from its arguments | The driver would guess which argument is the key; the lowering knows the operation's key inputs |
| Rename the MCP step's entry to the tool alone | The entry `surface/tool` keeps two surfaces apart in the plan; the file name is the replay's concern |
| Draft the free-text read into the holding folder with the mapping's question | The holding folder holds a file with the files that name what it defines; the read names the tool, not `surface/tool`, and a second question for one fact would be asked twice |
| Speak 2025-06-18 by offering 2025-11-25 and accepting the server's lower answer | The surface states the revision and nothing is probed (decision 0108) |

## Proven

- `a2m-pack-mcp` `tests/answers.rs`: `a_read_with_no_args_sends_the_key_its_mapping_names` (the stand receives `order_id`), `json_as_text_is_read_one_item_a_row_and_none_is_absent`, `a_write_is_read_back_through_a_tool_that_answers_json_as_text` (the stand's stored note equals the read-back's), `a_write_read_back_through_text_alone_is_refused_at_lint`, `a_saves_offline_request_is_recorded_under_its_fixture_files_name`, `the_earlier_handshake_revision_is_spoken_when_the_surface_states_it`.
- `a2m-pack-mcp` `tests/discover.rs`: `the_stands_tool_list_drafts_reads_that_read_what_it_stores` now also holds every reason non-empty and drafts `orders_get`'s read once the mapping answers `free_text: false`, which then reads what the stand stores.
- `a2m-provider` `replay::tests::a_save_is_sent_under_the_name_its_fixture_file_carries`.
- The stand: the drift `text_rows` and the setting `earlier_revision_only`, with `stdio_a_tool_can_answer_json_as_text_one_item_a_row` and `stdio_the_earlier_revision_is_served_by_handshake_and_can_be_served_alone`.

## Left

- A test through the product of an MCP write's offline test with its `request.json`, and of its publish, belongs to the product crate's tests and was not written here.
- The scout's summary still tells a person to settle an open question with `unknown.settle`, which answers `action_not_built` (finding 36). The text is made in `a2m-actions`, `src/handlers/scout_report.rs`.
- An operation declared `irreversible` over a tool with no read tool still needs the identity `none` to be acknowledged, as before (finding 39's second half): the compiler's rule of which writes need a read-back is not the pack's.
- No real server was run against these changes.
