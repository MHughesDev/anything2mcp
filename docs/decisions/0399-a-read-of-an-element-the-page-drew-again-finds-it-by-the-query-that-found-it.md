# 0399. A read of an element the page drew again finds it by the query that found it

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** `Element` in `a2m-pack-web-browser` keeps the `Query` that found it; the helper's `read` takes that query. The rule of [section 301](../spec/3-packs/301-the-web-pack.md) and of the crate's design that an element made stale by a re-render is never looked for again now holds for acting on it, and a read finds it again.

## Context

The base test `a_row_of_a_grid_that_draws_only_what_shows_is_found_by_its_key` failed in CI on Linux and Windows in one run, and here 4 times in 20, headless: the read of a cell of the row just located was `NotFound`. The grid page of the test application draws its rows in the handler of "Find row" and again on the `scroll` event that the same handler's change of `scrollTop` raises a frame later, each time removing every row and making new ones. When the second drawing came between `locate` and `read`, the row's element was gone. Made to skip a drawing whose rows are those already drawn, the page passed 20 times in 20, which shows the cause. The grid page was not changed by the runs between; the change of timing that made it show is not known.

A page that draws its rows again when nothing it shows changed is not what a keyed grid component does, but a page drawn from a template on every scroll does, and so does any page that renders a list again after a fetch. The base found the row by a query that names it by its key, and then could not read it, though the same query still found exactly one row. Every read the `ui` driver makes is a `locate` followed by a `read`, so on such a page a tool would fail now and then with no fault of its own.

## Decision

1. **An element keeps the query that found it.** `Page::locate` stores it in the `Element` it returns.
2. **A read of an element the page has removed reads the one element its query matches now, in the same call.** The helper's `read` finds it again only when the node is no longer connected to its document, and only in that same document: an element of an earlier document is gone with its document, and stays `NotFound`. Finding and reading are one evaluation in the page, so no drawing comes between them. A query that matches no element, or more than one, is `NotFound`, as before, and so is a query that cannot be matched from the element's own document, as for an element found through a frame.
3. **Acting on an element is unchanged.** An input, a check of it, a focus, or anything that sends is `NotFound` on an element the page drew again, and is never sent to the element found in its place: the element acted on must be the one that was found and checked. The caller finds it again, by role and name, as before.

*Instead of:* changing the test application's grid to draw only rows that enter or leave the view. It would have made this test pass and left the product failing on every page that draws its list again; the page's double drawing is something real pages do. *Instead of:* finding the element again in a second call from Rust, after a `NotFound`: a second drawing could come between that finding and its read again, and it would be a retry. *Instead of:* finding again for inputs too: an input sent to an element the step did not find and check is the silent re-query the rule forbids.

## Proof

`crates/a2m-pack-web-browser/src/tests/cases.rs`: `an_element_found_before_the_page_draws_again_is_stale_and_role_and_name_find_it_again` now reads the identifier of the button found before the list was drawn again and gets the new button's, while a click of it is still `NotFound`, and a read of an element whose query matches nothing after the row was archived is `NotFound`. `a_row_of_a_grid_that_draws_only_what_shows_is_found_by_its_key` passed 30 times in 30 headless and 30 in 30 with a virtual display server running and `A2M_REQUIRE_DESKTOP=1`, where before the change it failed 4 times in 20.
