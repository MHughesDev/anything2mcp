# 0399. The file drafter reads a sheet with no table and types a JSON field by its values, and the committing step carries what it writes

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Carries on [decision 0188](0188-credentials-over-plain-links-drafted-workbooks-and-published-device-maps.md) and [decision 0342](0342-a-documents-cell-may-be-empty-unless-it-is-a-key-or-declared-required.md) for the file pack, and settles findings 22 (the file half), 24, 25, and 26 of [field test 4](../reviews/field-tests.md).

**Date:** 2026-11-24

**Changes:** the file pack's discoverer (a workbook sheet that defines no table is a table found; a JSON list's keys carry the kind of their values), its drafter (a record kind placed by `sheet`, a JSON field typed by its values), and its lowering (the `save` step of a document and of a folder's file carries the arguments of the write); section 308 and the design of [`a2m-pack-file`](../architecture/crates/a2m-pack-file.md); the file formats reference, where a fixture's answers for the file pack are put in order. **Leaves alone:** the document format (the `sheet` place already existed), the plan's meaning of a step's arguments, the replay, and what the driver does at `save`.

## Context

Field test 4 ran the scout over a folder of an imaginary office's documents. Four findings were about the file pack's drafting and its offline tests.

- **24.** The drafted list of delimited text failed at the first empty cell. Decision 0342, made the same day, already makes every field that is not a key and is not `required` an output that may be absent; the list the scout drafted is built by that rule, and the test added here holds it across empty cells at the start, the middle, and the end of a row, and a row empty but for one cell. Nothing in the drafter needed changing.
- **25.** A workbook an open-source office suite saves has its rows under a header row on a sheet, and defines no table. The discoverer found tables only where the workbook defines one, so the drafted document held `records: {}` and no read was drafted.
- **26.** A JSON list of records was drafted with the type of every field an open question, though a JSON value states its own kind: a string, a number, true or false. The drafter gave every field of delimited text the type `untrusted_text` and left JSON for a person to type.
- **22, file half.** An offline test of a write compares the arguments of the committing step with the values its `<name>.request.json` states. The file pack's `save` step carried no argument (the key went to `locate` and the values to `set`), so a request holding the values failed and only `{}` passed.

## Alternatives

| | |
| --- | --- |
| **(a) A sheet that defines no table is drafted as the rows under its first row that holds anything, placed by `sheet`** | Chosen. The place already exists, and the reader already finds the header row by the declared headers |
| (b) Ask a person to declare the place of a sheet by hand | Rejected. It is how a person's spreadsheet is saved, so the scout would reach almost none of them, and nothing is guessed here: the header row is the first row that holds anything, and the read-time rule that it holds every declared header still stops a sheet laid out otherwise |
| (c) Draft the sheet as a table range | Rejected. A range is a position, and a row inserted above it moves it. The header text is the stable place |
| **(d) A JSON field is typed by the kind of every value it holds: text `untrusted_text`, a number `decimal`, true or false `boolean`** | Chosen. The file states it, as a workbook states each cell's kind |
| (e) Type every JSON field `untrusted_text` | Rejected. A number would be read back as text, and the type would be wrong for what the file holds |
| **(f) A JSON key whose values are of more than one kind, are lists or records, or are nowhere set stays an open question** | Chosen. This is what truly cannot be typed |
| **(g) The `save` step carries the key and the values the write sets, again** | Chosen. The step's arguments are what the engine hands the driver and what the replay records as sent; the driver reads none of them at `save` and saves the bytes `set` built |
| (h) Make the replay compare the `set` step's arguments | Rejected. The replay compares what the engine sends to a committing step, for every pack alike; a pack that wants its values compared puts them there |

## Decision

1. **A sheet of a workbook that defines no table, and holds a header row and at least one row below it, is a record kind of the drafted document**, named after the sheet, placed by `sheet: <name>`, with a field for each header the first row holds as text. Each column is typed by what the cells below hold, as a table's is. The fingerprint's `tables` lists only the tables the workbook defines.
2. **A JSON field is drafted by the kind its values hold**, across every record of the list: text as `untrusted_text`, a number as `decimal`, true or false as `boolean`; a `null` and an empty text hold nothing. Values of more than one kind, a list or a record inside, and a key that holds nothing anywhere keep their type as an open question. The key of a list of records stays an open question too: the file does not state it, and a path with a key is how the pack tells a list from one record. A field whose name is a secret's is left out, as before.
3. **The `save` step of a document carries the key's arguments and the fields' arguments of the binding** (`key.<field>` and `field.<field>`, as `locate` and `set` name them), and a folder's `save` carries those of its `set`. A `delete` carries the key alone. The driver does not read them at `save`. They are there so that the engine's evaluation of them is what an offline test compares with its `<name>.request.json`.
4. **Fixture answers for the file pack are consumed in the order the engine runs the steps**, written in [the file formats reference](../reference/file-formats.md) and section 308: one answer file named after the document (or the folder), a list in order, one item for each step of the file mode that is run for that document.

## Consequences

A drafted workbook of rows with no table compiles with a `list` read, as one with a table does; a person's spreadsheet is reached. A JSON document drafted by the scout holds one question for its key and one for each key it cannot type, not one for every field. A write's offline test can state the values it must send. The plan of a file write changes by the arguments of its `save` step, so a published plan's digest changes for a target that writes through the file pack.

## Tests

`stand.rs` in `a2m-pack-file`: `a_drafted_list_of_delimited_text_ends_at_the_end_of_the_data_not_at_an_empty_cell` (24), `a_workbook_saved_with_no_table_is_drafted_as_the_rows_under_its_header_row` (25, over a package built byte by byte in the shape the office suite writes), `the_fields_of_a_json_list_are_typed_by_their_json_type_and_only_the_untypable_is_asked` (26), and `the_committing_step_carries_the_key_and_the_values_it_writes_for_an_offline_test` (22).

## What would reopen it

A workbook whose first row that holds anything is a title and not the header row would be drafted wrongly; the read stops by name where a declared header is not in the first row that holds every one. Evidence that this is common would ask for a rule that finds the header row by its cells' kinds.
