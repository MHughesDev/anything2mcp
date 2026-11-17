# 0199. The halves left for each other are joined

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the designs of [`a2m-files`](../architecture/crates/a2m-files.md), [`a2m-actions`](../architecture/crates/a2m-actions.md), and [`a2m-local`](../architecture/crates/a2m-local.md); the `target.add` row of the action catalog in `a2m-actions` (an input `kind`) and the clients' generated actions; the agent guide's first step; one sentence of the [file formats](../reference/file-formats.md#testsyaml-and-fixtures). Adds `CompilerSet::address_key` and the lint code `kernel.acknowledged_with_lookup`. Takes up the lines decision [0196](0196-the-engine-and-the-registry-after-the-acknowledgement-and-the-first-hour.md) left for these crates, and its point 4, which it left not built.

## Decision

Most of this is adoption: the fields and the printing existed in two crates, and each waited for the other. Each point was first shown by a test that fails without it.

| Point | Built and test | Alternative weighed |
| --- | --- | --- |
| The pages print what the registry gives | The approval page prints a record's notices beside it, the count of records to write whose current values cannot be read, and `NOW_CANNOT_BE_READ` in the Now column as the program's words, set apart; the publish page prints every surface's kind and its address in each environment, named, its notices, and every warning. Every text is shown with the characters that reorder or hide text written as their escapes, then escaped as markup. `a_write_that_will_not_be_read_back_says_so_on_its_page_before_it_is_approved` (the keyed upsert with no read-back, through the real page), `a_records_notices_stand_beside_it_before_its_fields`, `the_publish_page_shows_where_each_surface_leads_its_notices_and_every_warning`; the first-hour test reads the surface's address on the publish page | Keeping the lines in `details` and having the page skip them: a page would have to recognise its own lines. The registry no longer adds them; the structured fields are the one place. The only person surface that shows acts is the local one |
| `target.add` with a surface | `CompilerSet::address_key(kind)`. Given `address` and `kind`, the target file holds one surface named after its kind, its address under the pack's key as a quoted scalar, and a test environment; it is compiled as a draft first, and a refusal names the compiler's errors and writes nothing. An address with no kind is refused with the kinds the enabled packs offer. A surface whose address is blank is refused by the compiler as stating none. `a_target_is_added_to_a_project_and_its_first_file_is_linted`; the first hour now writes no file by hand, in 6.2 seconds | Guessing the kind from the address: rule 7. Naming the surface by an input: no caller needs it yet; one more name to keep |
| An acknowledged keyed binding with a lookup | `kernel.acknowledged_with_lookup`, always a warning: the engine asks the lookup only whether the record landed; a lookup that returns the record's values is a read-back. The test pack's lowering takes `lookup: {call}`. No first-party pack's lowering makes such a binding (their compile tests pass) | An error: a lookup that only says whether a record exists is honest, as a take's look is |
| `fixture.record` beyond one pack's read | Two gaps found with the stand's reads: a list's items were scrubbed with no type, so a list of choices got made-up text its type refuses; and a choice's observed code was looked for in the files written, where another code stands for it, so the record always stopped. Items now take the list's field type, and a code is not looked for. `a_fixture_is_recorded_from_a_list_read_and_a_read_of_choices` | Refusing a list of choices: a plain read of statuses would stop |
| A preview's notices through the store | `a_records_preview_carries_the_notices_its_surface_kind_registers`, with the test application's pack registering its kind with a notice | — |
| A masked candidate through the store | Not proven. A masked value is never sent as a step's argument (`argument_masked`), so the test application cannot land a record whose recorded value is masked; it needs a step that records a value it does not send, a hook of the engine's test helper | — |

## Consequences

- A product test should record a fixture from the data pack's list read and the web pack's screen read; neither is tried.
- With two declared codes, "another declared code" is the other one: a person reading a scrubbed fixture can tell which code the system held. A code is the target's own vocabulary, not a business value, but a status may be one.

## Revisit if

Choices are held to be business data: then every code is put in place of by the type's first code, whatever it was.
