# 0474. A capture drafts a table with no name by its headings as a person reads them, and counts what the browser kept as one shape

**Status:** Accepted. Design decision, made by the worker on FT6b under the lead's brief. The owner had not read this record when it was written. No public contract changes; the screen model's `header` now holds a heading as a person reads it.

**Date:** 2026-11-25

**Changes:** the drafting rule of a table in `a2m-pack-web-ui` and its driver's read of a table's headings; how `web_use` in `a2m-pack-web-probe` counts the shapes of one screen's request; [section 301](../spec/3-packs/301-the-web-pack.md); the [file formats](../reference/file-formats.md) of `screens/`; the two crate designs; the test application's `/cases/rows-kept`. Carries out roadmap row FT6b, from field test 11, finding 62.

## Context

Field test 11's second application, a parts inventory, drafted nothing ([finding 62](../reviews/field-tests.md)). Its list of parts is a table that a table library fills: the table has no accessible name, and each heading carries the library's sentence for a screen reader (`Name Name: Activate to invert sorting`). The list's own `POST` to its screen's address was seen three times and counted in two shapes, because the library keeps its state in the browser and every load after the first sends more. So neither the screen nor the held page request of [decision 0392](0392-a-page-that-reads-by-a-method-that-may-write-is-learned-from-a-persons-watched-use-and-read-through-an-asserted-request.md) was drafted. Read from the code: the table rule skips a table with no name; a heading's words are its accessible name whole, and the driver compares the heading row's text whole; and `web_use` counts every distinct shape of a group, so any change at all makes two.

## Decision

1. **A table with no accessible name is drafted by its headings, held.** Where a part of the page holds one table with no name, the rule drafts it as it drafts a named one, named `{page}_table`, with its `rows` held to that part by the fingerprint, and its `locate` an open question that says the page gives the table no name and names the locator that finds it: `role: table` with `name: ""`. The driver looks for such a locator only inside its section (decision 0473, point 3), and every read holds the table to the headings the screen lists, so a table with other headings is never read in its place. Two tables with no name in one part stop by name.
2. **A heading is the words a person reads.** One closed rule, used by the drafter and by the driver alike: a heading with none of the words `sort`, `sorted`, `sorting`, or `sortable` is its own words; one whose words after its first colon hold such a word and whose words before it hold none is the words before it, with a run of them said twice said once (`Name Name: Activate to invert sorting` is `Name`); any other heading that holds such a word has no words of its own left. The drafter writes the words into `header`; the driver compares each heading of the table it reads by the same rule. A table one of whose headings has no words left stops by name: it is listed as not drafted, "no words of its own". The question of an unnamed table says when the sort words were taken off.
3. **What the browser kept is counted as one shape, where only that changed.** Requests of one method to one path from one page that came in two shapes are counted as one when the change is the one a page makes that keeps its state in the browser: every request of the first visit to the page is in one shape and no later one is; every request of every later visit is in the other; and the later shape differs only in fields it adds, to the body, to the answer, or to both, each field the two share being of one kind, the media type one, and no name withheld. The finding gives the later shape, with each field of the answer only it held marked optional, says `shapes: 1`, and says in a limitation that it was counted so. A change back, a change within one visit, a field of another kind, a body whose names were withheld after the first visit, or anything else that differs is still two shapes, and is not drafted, as before. The held page request is then drafted as decision 0442, point 6, says, with every field of the later body in its closed schema; a closed schema with no `required` takes the first visit's body too.
4. **The test application stands for such a page:** `/cases/rows-kept`, a table with no name whose headings carry the sort sentence in text hidden to the eye, filled by its script with `POST /api/cases/rows/kept`, whose body is `{draw, start, length}` on the first visit and adds the `order` the page kept in local storage on every later one.

## What this does not change

A named table, a heading with no sort word, and a group of one shape are drafted as before. A request seen once, or in shapes this rule does not join, is still not drafted. A page request drafted from a capture is still held with its effect asked; the person's word that it reads is still the only evidence it does ([decision 0392](0392-a-page-that-reads-by-a-method-that-may-write-is-learned-from-a-persons-watched-use-and-read-through-an-asserted-request.md), point 5). Trying an asserted read before its first publish is FT6d's, not this record's.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep the sort sentences of known table libraries in a platform profile | A list of libraries' sentences is longer, older, and narrower than the words they share; the closed rule names what it takes off and stops on the rest |
| Read a heading's visible text rather than its name | The snapshot holds the name the browser gives, and text hidden to the eye is still text; the base would need a second read of every heading |
| Count two shapes as one whenever the later adds fields | A read and a write by one address can differ by an added field; tying the change to the visit after the first is what a page that keeps its state does, and a write sent by a person on a visit does not |
| Leave a table with no name to be written by hand | It is the commonest table a table library draws, and the B screen FT6 was chosen for would still start from nothing |
| Locate an unnamed table over the whole page | Layout tables have no name either; the section is what tells it apart |

## Proof

`a_table_with_no_name_is_drafted_by_its_headings_as_a_person_reads_them_held` and `a_heading_with_no_words_left_once_the_sort_words_are_off_stops_by_name` (in `crates/a2m-pack-web-ui/tests/server_pages.rs`); `a_table_is_read_by_its_declared_headings` (the driver's unit test); `a_request_that_only_adds_what_the_browser_kept_after_the_first_visit_is_one_shape` (in `crates/a2m-pack-web-probe/src/tests/rules.rs`). Through the product: `a_person_s_two_visits_to_a_page_that_keeps_its_state_draft_its_unnamed_table_and_its_held_request` (in `crates/anything2mcp/tests/capture_use.rs`), on a virtual screen: a capture of two visits to `/cases/rows-kept` drafts the table's read, held, with the headings `Order` and `Customer`, and the held page request of `POST /api/cases/rows/kept`, seen twice in one shape, whose closed schema names `draw`, `start`, `length`, and `order`.

## Revisit if

A field test meets a table library whose sentence is not after a colon, or one that keeps its state by changing a value and not by adding a field; then the rule names that form, closed as this one is.
