# 0113. A screen's fingerprint is a section, absent is a variant, and a table is read by its headings

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the `screens/` part of [the file formats](../reference/file-formats.md); the design of [the web pack's drivers](../architecture/crates/a2m-pack-web-ui.md).

## Context

The file formats named a fingerprint's keys as `near_text`, `section`, and `role`, a variant's handling as `confirm`, `dismiss`, or `stop`, and `rows` as "how to find a row by key". None said how each is checked. The browser engine gives a query for exactly one element, optionally inside one other element, and nothing that says what role an element has or whether two queries found the same element.

## Decision

1. **A fingerprint is `section`, a locator of the one element the match must be inside, and optionally `near_text`, a text that section must show.** It is checked soundly with what the base has: the locator matched exactly one element of the page, so a match of the same locator inside the section is that element. *Instead of:* `role`, which cannot be checked without asking an element its role; it is not accepted, so that no file states a fingerprint the driver would not hold to. A locator with `within` cannot be under a fingerprint: the two scopes cannot be checked as one, and the compiler says so.
2. **An action states a fingerprint too,** as [spec section 301](../spec/3-packs/301-the-web-pack.md) requires; the file formats did not list it.
3. **A locator by role always states `name`.** `name: ""` matches an element with no accessible name. *Instead of:* a role alone meaning any name, which the base's exact match would not carry out.
4. **A variant's handling is `absent` or `stop`.** `absent` says the record the screen shows does not exist: a `wait` step that is a lookup reports it as absent, which is what a read-back needs. A write binding's `read_back` screen must declare one. `stop` is the application's refusal: `rejected`, `app_validation_error`. A variant is present when its `when: {visible: locator}` matches. `confirm` and `dismiss` wait for dialogs, which are not built. *Instead of:* taking absent from the status of the page's response, which many applications answer with 200.
5. **`rows` is the table, its `header` in order, and named `columns`.** The driver reads the text the page shows for the table, one line a row and one tab between two cells, and requires the heading row to be the declared one and every row to have as many cells. Anything else is `drift`. An output that is a list takes the column of its name. *Instead of:* locating each cell, which the base cannot do without a query for several elements. Finding one row by its key, and a table whose rows are not all in the page, are not built.
6. **Conditions built:** `visible`, `hidden`, `enabled`, `text`, `all`, `any`. `value`, `url`, `response`, and `profile_ready` are not accepted yet.
7. **A screen's `reach` is `url`, a path of plain segments and whole placeholders.** Each placeholder takes the operation's input of its name, as one percent-encoded segment; a value that is empty or made of dots is refused. No query, no fragment, no `from`.

## Revisit if

- The base gains a way to ask an element's role, or whether two queries found one element: then `role` returns, and `within` can stand under a fingerprint.
- A table is found whose cells hold a tab or a line break.
