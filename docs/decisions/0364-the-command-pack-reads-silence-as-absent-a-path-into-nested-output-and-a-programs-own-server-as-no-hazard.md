# 0364. The command pack reads silence as absent, a path into nested output, and a program's own server as no hazard

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md), from the findings 27 to 34 of [field test 5](../reviews/field-tests.md). The owner had not read this record when it was merged. The number is a placeholder that the lead renumbers on merge.

**Date:** 2026-11-24

**Changes:** [spec section 306](../spec/3-packs/306-the-command-pack.md) ("The commands file", "Output", "Effects and hazards", "The scout"); the design of [`a2m-pack-command`](../architecture/crates/a2m-pack-command.md).

## Context

Field test 5 ran the pack against a widely used key-value store's own command-line client. Seven of its findings are the pack's. Four of them need a choice between real designs: a program that answers "none" with silence (30), a write to a program's own server that the pack forced to `irreversible` (32), a JSON document that holds a header and a list (34), and a list that answered nothing through the engine (31). The others are faults with one right answer (27, 28, 29, 33).

## Decision

1. **A read may declare that printing nothing means absent: `absent: empty_output`.** A command that is a `read` and declares it, when it ends with an `ok` exit and prints nothing but white space, has found no record. It means what an exit status declared `absent` means, and has the same two ways out: the look before a write answers "absent" (so an update of a key the program does not hold stops before it is put to a person, with nothing written), and a read of that key answers `record_absent`. Without the declaration, silence is output that does not parse, as before. A command that is not a read may not declare it (`command.absent_empty_needs_read`): a write that printed nothing is not known to have done anything. *Instead of* a default that read silence as absent for every command: most programs print nothing only when something is wrong, and a pack that took that for "none" would turn a broken pipe into a missing record.

2. **A program's own server is declared as `reaches: {own_server: [host, ...]}` and is not a hazard.** `reaches: {network: [...]}` keeps its meaning: the program sends something to a place outside the target, so the command declares `hazard: sends_outside` and is `irreversible`. A command whose program talks to the server that holds the target's own records declares that server under `own_server`; it stays a `read` or a `write`, and the kernel's rule of which writes need a read-back is the one that decides whether it can be verified. The declaration is kept in the plan, so the review can show it, and each entry is a host name with no scheme, path, or space (`command.own_server_malformed`). The pack cannot check that the server really is the target's own: the declaration is the developer's, as `network` is, and the listing says `declared`. *Instead of* letting `network` mean both: a write the pack calls irreversible is never verified, so the honest declaration was worse than leaving it out, and the design gave people a reason to leave it out. *Instead of* no declaration at all: the review would not show that the program reaches a server.

3. **A JSON output may name the path to its records: `json: {at: [items], fields: {...}}`.** The keys are followed from the document down; what is found there is the list of records, or the one record. Everything else in the document is not read, and is not drift: the path is the declaration of where the records are. A path that does not lead there (a missing key, or a step through something that is not an object) is a failure as any other output that is not of its form. `at` and `lines: true` do not go together (`command.output_malformed`). Values that the program encodes inside the records are still read as they are printed: decoding them is a separate question, which this record leaves open. *Instead of* a path expression: a list of keys is what the findings needed, and a language for paths would be a second thing to keep true.

4. **A list gives each declared field as a list, and keeps the rows under `rows`.** The engine fills an operation's list outputs from the field of that name, one value for each row, as the data pack gives them. The command pack gave only `rows`, so a list over a command answered `{"sku": null}` (finding 31). A field named `rows` is the field's list. A list with no row gives each field an empty list. The fault was in the pack's step result, not in the engine.

5. **The version is the line the pattern matches.** The pattern is laid over the whole output first, as before; if it does not match, over each line, and the version is the one that the matching lines agree on. Two matching lines that disagree are no version (unknown means stop). A program that prints its interface's version on a second line no longer stops the scout as `program_version_unsupported` (finding 28).

6. **A help listing is read with a tab or two spaces between a name and its text, and lines that are skipped are said.** The name ends at the first run of two spaces or the first tab. A line that holds no name a reader can take is counted, and a finding of kind `command_file` about the program's help (`form: program_help`) says how many lines were read, how many were not, and the reason (`name_not_plain`), at partial coverage. The scout's summary, which is in `a2m-actions`, can show the counts of that finding; the pack gives them as data (findings 29).

7. **A failure quotes whole lines of standard error, and keeps its line breaks.** The text is redacted whole first, as before. The quote is the last lines that fit within the bound the doors show (`SAID_LONGEST`), each whole, in their order, with a first line that says how many earlier lines were left out; the cause of a failure is usually printed last, and a warning first. A single line longer than the bound is cut, with `…`. If taking the control characters out would put a secret together that one of them divided, the pack falls back to quoting the flattened, redacted text, as before (finding 33).

8. **Finding 27 is partly the pack's and partly `a2m-actions`'.** The pack's part is that discovery stops with `program_digest_changed` where the person declared a digest and no build was written; that is a message in the discoverer and the next step the product names after `target add` of a `program` surface, which `a2m-actions` owns. This record does not change either; field-tests.md says where it belongs.

## Proven

- `output.rs`: `a_path_reaches_the_records_in_a_document_that_holds_more`.
- `driver.rs` (unit): the version of the first of two lines, and two lines that disagree.
- `discover.rs` (unit): `a_listing_set_out_with_tabs_is_read_and_a_skipped_line_is_counted_by_reason`.
- Against the stand `example-cli` (new faults `TabbedHelp`, `UnnamedHelpLine`, `SilentMiss`, `InterfaceLine`, `NestedJson`): `tests/metadata.rs` `a_second_version_line_and_a_listing_set_out_with_tabs_do_not_stop_the_scout`; `tests/driver.rs` `a_read_that_prints_nothing_is_absent_when_its_command_says_so_and_not_otherwise`, `a_list_gives_each_declared_field_as_a_list_for_the_operations_outputs`, `a_nested_document_is_read_at_the_path_its_command_declares`, `a_failure_quotes_whole_lines_of_standard_error_within_the_bound`; `tests/files.rs` `a_write_may_declare_the_servers_that_are_the_targets_own_and_stays_verified` and the three refusals of the rule test.

## Left

A test of a command's list through the engine and the product, which no crate of this package owns: the pack's step now returns what the engine reads, and the product's test of a list over a command is for the owner of the product crate. The scout's summary of a skipped line is in `a2m-actions`.

## Revisit if

A program prints its records in a document whose path is not a fixed list of keys, or a field test finds a server that is the target's own and still must be `irreversible`.
