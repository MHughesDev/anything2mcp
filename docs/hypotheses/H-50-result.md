# H-50: pages that ask for their own data by a method that may write are a large share of the web applications a person uses

**Status:** refuted, by the outcome the counting rule fixed beforehand: one application of eight has a screen outside class A.

**Tested:** 2026-11-25, before package FT6, by the counting rule written in the register on 2026-11-24. The owner gave the go-ahead for this session to run self-hosted open-source applications in local containers on the test box, with no real account and no live service (rule 10). The names of the directory, the applications, and their images are in the recipe folder outside this repository (rule 9).

## What was assumed

Pages that ask for their own data by a method that may write are a large enough share of the web applications a person uses that the web path is not reached without them. The alternative: they are few, or nearly all of them send a read and a write to one address in bodies that differ only in a long text, so that only a pin by digest (package E9) would open them.

## What was done

The counting rule's steps 1 to 7, in order.

1. **The pool.** One public list of self-hosted software, kept on a code-hosting site, read at one commit on 2026-11-25. Its five sections that match the rule's categories: project and issue tracking (a software project management section), customers (a customer relationship section), stock (an inventory section), wikis, and help desks (a ticketing section).
2. **The choice.** Each entry was judged against the copy test's choosing rule from its own documentation, read with `curl`, and nothing else. "One container" was read as: the application and its store in one container, an embedded database or files; an entry whose documentation needs a database server or another service beside it fails. "Pinned by digest" was read as: the project publishes an image on a registry; an image a person must build from source fails. Three entries could not be judged because their documentation could not be read from the test box (refused by the network proxy, rate-limited, or not found); they were skipped and are counted below. **The list of ten was written at 2026-11-25T13:26Z, before any was started.** It held nine, not ten: in the customers section only one entry meets the rule, since every other needs a database server or several containers.
3. **Replacing.** Five of the nine did not start because their image host was not reachable from the test box: three on one code-hosting site's container registry, whose blobs the proxy refuses, and two on project registries the proxy refuses. Each was replaced by the next entry of its section that meets the rule. One replacement, in stock, was on the same refused registry, and the section had no further entry that meets the rule. The images that were fetched came through a public mirror of one registry, each pinned by its digest.
4. **What was looked at.** In each of the eight that started, signed in with an account made up in the local container, the first screen in the application's own navigation that lists records of its main kind, and the screen of the first record on it. The requests each page made by itself were read through the browser's own network events in Playwright on Chromium, standing in for the developer tools, with the body of every request that is not a `GET` read through the browser's protocol, since Playwright's own reading of a `fetch` body came back empty for some. Then one record was created, changed, and deleted through the application's own screens, and those requests were read too. Each recipe (image digest, seed commands, sign-in, the two screens' addresses, the scripts) is in the recipe folder.

| Slot | Category | Kind of application | Started |
| --- | --- | --- | --- |
| 1 | projects and issues | forge with issues; pages drawn on the server | replacement, the first choice's registry refused |
| 2 | projects and issues | code review; one-page application over a JSON interface | first choice |
| 3 | customers | low-code records platform; one-page application over a JSON interface | first choice, the only entry that meets the rule |
| 4 | stock | parts inventory; pages drawn on the server, tables filled by a table library | replacement, the first choice's registry refused |
| 5 | stock | none | the first choice and its replacement both on a refused registry; the section was exhausted |
| 6 | wiki | flat-file wiki; pages drawn on the server | first choice |
| 7 | wiki | wiki with a database; pages drawn on the server, a JSON interface for its scripts | replacement, the first choice's registry refused |
| 8 | help desk | error tracker; one-page application over a JSON interface | replacement, the first choice's registry refused |
| 9 | help desk | issue tracker; pages drawn on the server | first choice |

## What was found

**The counts.** Sixteen screens, from eight applications:

| Class | Screens |
| --- | --- |
| A: records in the document or by `GET` alone | 15 |
| B: another method needed, told apart from every write | 1 |
| C: a read and a write differ only in a long text or not at all | 0 |
| D: records only after a press, or over a socket or a stream | 0 |

- Applications with both screens in A: 7 of 8.
- Applications with a screen outside A: 1, the parts inventory.
- Entries not started: 6 (5 first choices and 1 replacement), all because the image host was not reachable from the test box. Replacements made: 4. Slots left empty: 2, one in customers (no second entry meets the rule) and one in stock (no entry left).
- Entries not judged, because their own documentation could not be read from the test box: 3.

**The B screen.** The parts inventory's list of a category's parts arrives by `POST` to the screen's own address, with the form body `_dt=dt&_init=true`; later pages of the table add the order, the start, and the length. No create, change, or delete used that method and path: they went to `/part/new`, `/part/{id}/edit`, and `/part/{id}/delete`. So it is B by the rule's first branch. The body has no field whose short values tell a read from a write; nothing needs one, since the address takes no write.

**Two judgments the count rests on.**
- *The parts inventory's record screen was counted A.* The record's fields are in the document. A history table on the same screen arrives by the same kind of `POST` as the list, to the record's own address. Counted strictly as "every request the screen needs", it would be B, and the counts would be 14, 2, 0, 0, with still one application outside A.
- *The flat-file wiki's list screen was counted A.* The first entry of its site tools, the list of recent changes, lists pages in the document. Its third, the site map, shows the pages of a namespace only after a press, by `POST` to a shared endpoint with a `call` field. Had the site map been taken, that screen would be D.

**What the count does not show, and the product meets anyway.** On loading the second wiki's list of recent changes, the page's own script sent four `POST` requests that save the signed-in user's display preferences. The screen is A by the rule: its records are in the document. But an observing tab fails every request that may write, and the web pack's drivers turn any request the tab failed into the stop `page_write_refused` (`refusal` in `crates/a2m-pack-web-ui/src/driver.rs`). So a read of that screen would stop today, though nothing it needs is a write, and an asserted read cannot help, because those requests do write. The parts inventory's record screen would stop the same way for its history table, unless the history request is asserted. Neither was run through the product; the reading is of the code.

**The writes.** Every create, change, and delete went to an address of its own or by its own method: a form `POST` to a new-record or record address (five applications), a JSON `POST`, `PUT`, `PATCH`, or `DELETE` on a record address (three). One wiki deletes a page by saving it empty, through the same request as a change. No write shared a method and path with a read, so no screen came near class C.

## What changes

H-50 is marked `refuted` in the register. By step 7:
- Fewer than two applications have a screen outside A, so the bet is refuted, and package E7 was built for few.
- Screens in C (0) are not more than screens in B (1), so E9 stays after E8, as the roadmap has it.
- Screens in D (0) are fewer than four, so H-53 does not get its spike before FT6.
- FT6 runs on the first application of the list with both screens in A (slot 1) and the first with a screen in B (slot 4). Their recipes are in the recipe folder.

The roadmap's FT6 row names how the two were chosen. No order in the roadmap changes, so no decision record is written.

## What is still unknown

- **Ten were not counted.** Eight applications started, not ten: the customers section has one entry that meets the rule, and four images of the stock and other sections were on registries the test box cannot reach. The share of applications with a screen outside A is 1 in 8 here; on a box that reaches those registries, the four lost entries, three of them single-container applications with embedded stores, could change it by one or two.
- **The choosing rule shapes the pool.** An application that fits in one container with an embedded store is more often small and drawn on the server; the large ones, which need a database server, were all left out. Whether pages that read by `POST` are more common among those is not shown.
- **A page that writes by itself while it is read.** One screen of class A sends writes of its own on load. How often this happens, and whether an observing tab should fail such a request and go on rather than stop, is not counted by this rule and not yet a bet; it is a question for FT6 and for decision 0195's design. If FT6 meets it, it is a new hypothesis.
- **Filters and paging.** Each list was read as first shown. The B screen's later pages and a filtered list send a body that changes with the input; whether that needs package E8 was not tried.
- **The judgments above.** Two screens could each have been classed otherwise; the outcome holds either way.
