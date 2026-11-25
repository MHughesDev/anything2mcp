# 0999. A desktop table of cells is read by the column count it states, a table with no name by its headings, and a title two windows show to differ becomes a pattern

**Status:** Accepted. Design decision, made by the worker on field test 10's findings under the lead's brief. The owner had not read this record when it was written. The `screens/` format of a `desktop` surface gains two optional forms, each a refusal before; no public interface of the kernel changes.

**Date:** 2026-11-25

**Changes:** `a2m-pack-desktop` (the Linux back end, the driver, the tree, the discoverer `desktop_snapshot`, the drafting rules, the `screens/` file kind and the lowering, the payload format 3); test stand 28 (`targets/example-desk`, a window of one record); [spec section 305](../spec/3-packs/305-the-desktop-pack.md); [the pack's crate design](../architecture/crates/a2m-pack-desktop.md); [file formats](../reference/file-formats.md); the [field test results](../reviews/field-tests.md), findings 57 to 60; roadmap FT5 and its row in the [Build Order](../plan/build-order.md).

## Context

Field test 10 read a GTK 3 dialog program on Linux through the desktop pack ([decision 0459](0459-the-field-test-box-takes-a-desktop-session-and-one-application-the-recipe-names.md)) and left four findings, all the pack's:

- **57.** The toolkit publishes a table with no name whose column headers and cells are its own children, with no `table_row` between. The driver read a table's rows only as `table_row` elements and stopped with "holds something that is not a row"; the drafter drafted a list only of rows of cells; and a locator needs a name, so a person could not write a screen that finds the table either. The stand published rows, a shape this toolkit lacks.
- **58.** For that window the scout listed the one finding with "the rule gave no reason". (That it was listed twice is `a2m-actions`' and was fixed on staging before this work: the scout report lists a finding once, with each reason once.)
- **59.** A labelled spin button, which offers its value as text, was left out of the drafted read without a word.
- **60.** The window was reached by its whole title, "Order 1001", which names the record it shows; the screen, the operation, and the tool were named after it (`orders_order_1001_get`), and the read found a window only while that one record was shown.

The web probe already solves the shape of 60 for a page: two looks at pages whose paths differ in exactly one segment, with one shape, are one screen whose path holds a placeholder, and one look never is ([decision 0445](0445-the-first-hours-faults-a-list-on-the-command-line-a-key-off-a-list-and-a-project-held-to-one-store.md)).

## Decision

1. **A table whose cells are its own children is read by the column count its system states, and only then.** On Linux the back end asks a table that offers AT-SPI's `Table` interface for `NColumns` (added to the read list of properties; asked of a table only). The first that many children must be column headers, which are the headings; each run of that many cells after them is one row. A table that states no count, whose cells are not a whole number of rows, that does not begin with one heading per column, or that holds rows and something else beside them, is a stop, `fingerprint_mismatch`, naming why: nothing tells where a row ends, and nothing is guessed (rule 7). A table of rows is read as before. On Windows and macOS no count is read, and a table is read by its rows only, as before.
2. **A table with no name is located by its headings.** `rows` may leave out `locate`; the table is then the one of the window whose headings, read as point 1 reads them, are `header` exactly. Two such tables are `locator_ambiguous`; none is `locator_not_found`, or, where a table of the window cannot be read, the `fingerprint_mismatch` that says why. The drafter drafts the one table a window gives no name as the screen `{window}_table`, without `locate`, ready when its first column header is shown.
3. **Every finding the window rules draft nothing from says why**, as the web pack's rules do: the window cut short, no target name, no one `desktop` surface of the program, a title that cannot name or reach a screen, a placeholder named as a secret, nothing a screen could locate, only values that give no text, or screens that hold a question.
4. **A labelled spin button is drafted as a value when it gives its value as text.** A snapshot's element now says `"text": true` when it does, and a table's says `"columns": n` when it states its count; both are the window's shape, never a value. A spin button that gives no text is left out, and the drafted read's description names it. Its value is read as text, `untrusted_text`, as an entry's is.
5. **A window title two windows show to differ in one word becomes a pattern, and one title never does.** Within one snapshot, the discoverer matches windows of one role and one shape (the window's own element, named by its title, aside), each walked whole, whose titles have the same number of words and differ in exactly one, a plain word (letters, digits, `-`, `.`, `_`) that is not the same in all. A window two groups would hold is in neither; a title of the differing word alone, or holding a brace, is never matched. The group is one finding, its title the words with `{name}` in place of the one that differs, `name` made as the web probe makes it (the word before, as one record of it, with `_id`, else `key`), and `windows` its count.
6. **A screen's window title may hold one placeholder.** `title: "Order {order_id}"` finds the window whose title is exactly the words around the placeholder with the value of the operation's required input of that name between them; `goto` carries it as its argument `title`. A placeholder that no required input fills is `desktop.title_unbound`; a second one, or a title that is a placeholder and nothing else (which would match every window), is `desktop.title_invalid`. A part of the record's key that the title holds is proven by `goto` and needs no field. The drafted reads over such a window take the placeholder as their one input and their key, and are named after the title's other words: `orders_order_get` with `order_id`.
7. **A write through a screen whose title has a placeholder is refused, `desktop.title_write`.** Before every act the driver proves again that the window shows the record, by a field; with a pattern it would have to prove the title again too, which is not built. This limit is a "not built yet".
8. **A drafted read of a window found by a whole title with a digit in it says how to make the title a pattern:** open a second record's window beside it and scout again. Nothing else changes for one title.
9. The compiled screens are payload format 3.

## Consequences

- Field test 10 run again on this branch, three runs of the form and one of the table: the table of three orders is drafted as `orders_open_orders_table_list`, the scout lists nothing as not drafted, and a try returns the three orders, customers, and line counts; the form's read holds `lines`, `"3"`, beside the customer and the city, in all 17 steps of each run. The dialog program shows one window per process, so the field test cannot show two windows of one record's shape; finding 60 is proven on the stand, with two windows of one record each, where the drafted read with `order_id` reads each window's own values and an order no window shows stops as `unexpected_page`.
- The screen matching of point 5 is written a second time, after the web probe's (`placeholder` is the same rule over words in place of segments). A count for H-37: about 120 lines repeated.
- A pattern across scouts at different moments (one record now, another later) is not built: the scout matches what one snapshot shows.

## Alternatives considered

- **Rows from cells by a guess**, such as the number of headers alone, with no count stated. Rejected: a table whose header count and column count differ would be read into wrong rows, silently (rule 7).
- **A locator with an empty name, or by position among the window's tables.** Rejected: an empty name matches every unnamed element of the role, and a position moves when the window changes; the headings are already declared and checked by every read of the table.
- **A pattern from one title, by its digits.** Rejected: a title's number may be a version, a count, or a part of the application's name; one title shows nothing of what differs between records. The description of point 8 tells the person instead.
- **Building writes over a patterned title now.** Deferred: it needs the title proven again before every act, with its fault-injection tests.

## What would reopen it

A toolkit that publishes a table of cells with no row and states no column count, which this decision cannot read; a window whose record's key is in two words of its title; or an application whose windows of one screen differ in shape by a control shown for some records only.
