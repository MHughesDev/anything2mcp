# 0999. An asserted page request is never sent by the engine, and is handed to the tab only from a published version

**Status:** Accepted. Design decision, made by the worker who built package E7c, under the lead's brief. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** the `page_requests/` file kind (`page_request`), its lint and lowering, the `ui` and `page_http` drivers, and the next step of `page_write_refused`, all in [`a2m-pack-web-ui`](../architecture/crates/a2m-pack-web-ui.md). It builds points 2 and 4 of [decision 0394](0394-an-asserted-read-of-a-web-surface-is-a-page-request-matched-by-one-shared-shape-and-shown-at-publish.md), and settles what those points left to the builder.

## Context

Decision 0394 says that `read_asserted` is a key of the page request, that an asserted read gives a closed `request_schema` or none, and that the drivers hand the observing tab the asserted reads of a published version under decision 0270's rule. It does not say whether the `page_http` mode may send an asserted read itself, which of the shape's words its body takes, how a form body is checked at compile time, or what "live" means for a compiled target that is not published. Each is a choice between real alternatives; each was taken toward the narrower reading.

## Decision

1. **No mode sends an asserted read.** The `page_http` mode reads by `GET` alone through the engine's `read_from_page`. A `page_http` binding, or a `read_back`, that names an asserted read is refused at lowering as `web.request_method_not_built`, and the driver refuses one again at run time as `step_not_usable`. An asserted read is let through the tab when the page sends it, and nowhere else.
2. **An asserted read's body takes `type` and `enum`.** Its `request_schema` is an object with `properties`, `required`, and `additionalProperties: false`; each property states a `type` other than `any`, and a string may state an `enum`. `nullable`, nesting, and the other words of the shared shape are refused, as for every page request. A response's shape still takes no `enum`, since the compiled answer keeps no texts to check one against. With `request_encoding: form`, every field is a string, because a form's values are texts and a field of another kind would match nothing. `request_encoding` with no `request_schema` is refused as `web.read_sends`; so is `anti_forgery` and `responses.refused`.
3. **`read_asserted` is refused where it means nothing.** On a read by `GET` and on a write, in any value, as `web.request_effect_mismatch`. `read_asserted: false` on a read by another method leaves it unasserted, which then does not compile. Any method other than `GET` that the file kind takes may be asserted. The file kind's `writes` answers true for any entry that carries the key, so a drafting rule never writes one.
4. **A live run hands them only from a published version too.** The HTTP pack lets an asserted read through in any live run; here the list is filled only when the plan's version is not the draft version (as `published` in `crates/a2m-pack-http/src/driver.rs` decides it), and then in a live run, or in a try or a shadow whose environment is of kind `test`. Discovery is never given it, and the web probe keeps handing an empty list.
5. **The stop names the page request.** When the tab fails a request whose method and path, segment for segment, an asserted read of the surface names, the next step names that page request: under the drafts, or in a posture that is not given the list, it says to publish and run from the published version; where the list was given, it says that what the page sent did not fit the entry. The path is matched as sent and shown only by structure.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Let `page_http` send an asserted read by `POST` | A second path for a request the person's word alone vouches for, sent by the engine rather than by the page; the browser engine's `read_from_page` sends `GET` alone, and widening it is another crate's change |
| Take every word of the shared shape in an asserted body | The tab checks the body against the shape exactly, so it would be safe, but a page request does not take them yet, and the `enum` is the one the match needs |
| Leave a form body's kinds to the browser engine, which fails a non-text field at run time | A target that compiles and can never read is a stop found late; the compile says it now |
| Follow the HTTP pack and hand the list in every live run | A compiled target under the drafts is not run live through the product, so nothing is lost, and the rule is then one sentence for all postures |

## Proof

`crates/a2m-pack-web-ui/tests/asserted.rs`, against the test application's case 39: a read by `POST` with no `read_asserted`, with an open or default-open `request_schema`, with an encoding and no body, with `read_asserted` on `GET` or on a write, with a form field that is not text, with a token, and through `page_http` does not compile; one with no body does. A `ui` list of `/cases/rows-by-post` under the drafts stops as `page_write_refused`, live and in a try, with a next step that names `cases_rows_search` and says to publish; from a published version it returns the open rows the control interface holds, live and in a try in the `test` environment; a try in the `production` environment and a discovery stop; the same page with no asserted read stops with no page request named; an `enum` the body does not fit stops and says so; a second page's own write, which no entry asserts, still stops the read and reaches nothing. A `page_http` read from inside the same page stops under the drafts and reads from a published version. The control interface shows no row changed, a search heard for each read let through, and no `archive` heard at all.

## Revisit if

The provider contract gains the mark that a plan is a published version's (decision 0270, owed), which replaces the version test; or a page is found whose asserted read must carry a body field nested or nullable.
