# 0473. A capture drafts a list of items of one shape and a record headed by its value, held

**Status:** Accepted. Design decision, made by the worker on FT6a under the lead's brief. The owner had not read this record when it was written. It changes the screen model's file form (a locator by `heading`, `rows` read as `items`) and the browser base's two public enums (`By::Heading`, `ReadWhat::Items`), which are the web pack's own.

**Date:** 2026-11-25

**Changes:** the drafting rules of a `screen` finding in `a2m-pack-web-ui`; the matching of a screen's pages in `a2m-pack-web-probe`; the `ui` driver's `find` and its read of `rows`; `By` and `ReadWhat` in `a2m-pack-web-browser`; the [file formats](../reference/file-formats.md) of `screens/`; [section 301](../spec/3-packs/301-the-web-pack.md); the three crate designs; the test application's `/orders/listed` and `/orders/{order_id}/record`. Carries out roadmap row FT6a, from field test 11, finding 61.

## Context

Field test 11 captured a person's use of a forge whose pages are drawn on the server, and nothing was drafted ([finding 61](../reviews/field-tests.md)). Its list of issues is links in a list, not a table, and its record page shows the issue's title as its one heading and the rest with no label. The drafting rules read only a table the page names and a value beside a term of the same name ([decision 0183](0183-the-web-pack-drafts-a-read-of-each-table-a-snapshot-names.md), [decision 0187](0187-the-web-pack-matches-a-screen-by-its-address-and-reads-by-key.md)). Three facts were read from the code:

1. **A record page was never joined with its kin.** The probe joins pages whose addresses differ in one segment only when their named tables or labelled values agree; a page with neither has no signature and is never joined, so `/issues/2` and `/issues/3` were two screens of one page each, and nothing said the heading was a value.
2. **No locator could find what such pages show.** A list with no name is `role: list, name: ""`, which the driver first looks for over the whole page, where a page's navigation lists share it; a heading whose name is the record's title has no fixed name to be found by. The person wrote a `css` locator.
3. **A table's read is its text, cut at tabs and lines.** A list's text is not one line to an item when an item holds more than one block.

## Decision

1. **A list of items of one shape.** In a `screen` finding, the one list with no accessible name inside a part of the page about its content (`main`, or a named `region`), not inside a table or another list, with two items or more, each a `listitem` right under it, and each item repeating one shape (the roles and depths below it), is drafted as a screen `{page}_items` whose `rows` read the list as `items: listitem`, with one column, `items`, and a `list` read `{target}_{page}_items_list` whose one output is that column. Its `locate` is an open question that says the page gives the list no name and names the locator that finds it: `role: list` with `name: ""`, held to its part by the fingerprint. A list whose items differ in shape, or that holds something that is not an item, stops by name: it is listed as not drafted, "items that differ in shape". Two such lists in one part stop by name as well.
2. **A record headed by its value.** The probe gives a page that holds neither a named table nor a labelled value, and does hold a heading, a signature of a record: every element's role and depth, a heading's level, and the name of each part of the page, and no other name. Pages whose addresses differ in one segment and that share it are one screen, as before; in the snapshot it keeps, of the first page, each heading whose name the pages do not share is kept as its type and marked `varies`. The drafter reads such a screen as a record: the one varying heading, the one heading of its level in a part about content, and every paragraph of that part, each right under it, are drafted as a screen `{page}_record` with a field `heading` and `rows` read as `items: paragraph` with one column `values`, and a `read` `{target}_{page}_record_get` whose outputs are the heading and the values, as one list in the page's order. The heading's `locate` is an open question that says the heading is the record's value and the values below it have no labels, and names the locator `heading: N`. Where a heading varies and the record is not of that form, it stops by name: "the record cannot be read by rule". One page alone is never read as a record: only two pages that show different headings say the heading is a value.
3. **What the driver reads them by.** A locator `heading: N` matches the one heading of that level (`By::Heading` in the base). `rows` with `items: listitem` or `items: paragraph` read the text of each element of that role inside the element found, and not inside another of it, each with its white space collapsed, one to a line (`ReadWhat::Items`); each is one row of one cell, so an item that spans blocks is still one row. A locator by role with `name: ""`, and one by `heading`, under a fingerprint is looked for only inside the fingerprint's section, where it must be the one; and a locator that is its own section is that one element of the page, with no check that it is inside itself. Every other locator is found as before: once over the page, then held inside its section.
4. **Held, saying why.** Neither a list nor a record is drafted where a person has not said its locator stands: each waits in the holding folder on its question, with its read held with it ([decision 0298](0298-a-draft-with-an-open-question-waits-in-a-holding-folder.md)). A person releases it by writing the locator the question names, or a better one, in its place and moving it out. A draft of a capture also asks for `ready` ([decision 0442](0442-a-persons-use-is-watched-by-web-use-and-what-it-saw-twice-is-drafted-held.md), point 8).
5. **The test application stands for such pages:** `/orders/listed`, the orders as a list of links, one to an item, and `/orders/{order_id}/record`, an order headed by its customer with its id and its status below as paragraphs with no label; `GET /__control/orders` gives every stored order.

## What this does not change

A table the page names and a labelled value are drafted as before; their pages are joined as before. A link's name in a list's item is still data and is kept as its type. No locator by position, and none by XPath. Nothing here drafts a write. What the outputs are called (`items`, `heading`, `values`) is provisional, as every drafted name is.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Read a list by its text, cut at lines | An item of two blocks becomes two rows, and nothing on the page says so: a guess (rule 7) |
| Draft a `css` locator for the heading and the list | The last resort, flagged by lint, and a tag is not the role the snapshot saw |
| Name each value of a record by its place (`value_1`, `value_2`) as a field of its own | A locator by place is not accepted, and a name by place says nothing a list in order does not |
| Read a single record page as headed by its value | One page cannot say whether its heading is a title like "Orders" or a record's value |
| Draft them unheld, since a read is checked against the page | The evidence is weaker than a name or a label, and the brief's rule is that such a draft waits on a person's word |
| Locate an element with no name over the whole page, as every locator is | A page's navigation lists have no name either, so the locate would be ambiguous on most pages drawn on the server |

## Proof

`a_list_of_links_and_a_record_headed_by_its_value_are_drafted_held_released_and_read` (in `crates/a2m-pack-web-ui/tests/server_pages.rs`): the findings the probe gives of the two pages are drafted held, each saying why; a person's release compiles; the list's read through the `ui` driver returns every order id `GET /__control/orders` gives, and the record's read of `A-101` returns its customer as the heading and its id and status as the values, with no write. `a_list_whose_items_differ_in_shape_stops_by_name`. The probe's join: `a_record_s_pages_are_joined_and_the_heading_they_do_not_share_is_kept_as_its_type`. Through the product: `a_person_s_use_of_pages_drawn_on_the_server_drafts_a_list_and_a_record_held_that_a_person_releases_and_tries` (in `crates/anything2mcp/tests/capture_use.rs`), on a virtual screen: a capture of the test application's person walking the list and two records drafts both reads held, a person releases them, and `draft try` of each returns what the control interface holds.

## Revisit if

A field test meets a list whose items are not `listitem` elements (a list drawn with `div`s and no roles), or a record whose values are not paragraphs; then the rule names the next shape, closed as this one is. Or the join of records joins two pages of different kinds that share one structure; then the signature gains what tells them apart.
