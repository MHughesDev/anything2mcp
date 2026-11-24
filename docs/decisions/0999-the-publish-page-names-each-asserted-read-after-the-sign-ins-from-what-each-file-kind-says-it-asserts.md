# 0999. The publish page names each asserted read after the sign-ins, from what each file kind says it asserts

**Status:** Accepted. Design decision, made by the worker who built package E7d, under the lead's brief. The owner had not read this record when it was merged. It changes a public contract by addition: the file-kind interface gains a method with a default, and the review summary gains a field.

**Date:** 2026-11-24

**Changes:** [`a2m-files`](../architecture/crates/a2m-files.md), whose `FileKind` gains `asserts` and whose review summary gains `asserted`; [`a2m-local`](../architecture/crates/a2m-local.md), whose publish page shows them; the `page_request` kind of [`a2m-pack-web-ui`](../architecture/crates/a2m-pack-web-ui.md) and the `endpoint` kind of [`a2m-pack-http`](../architecture/crates/a2m-pack-http.md), which answer. It builds point 5 of [decision 0394](0394-an-asserted-read-of-a-web-surface-is-a-page-request-matched-by-one-shared-shape-and-shown-at-publish.md) and settles what that point left to the builder.

## Context

Decision 0394, point 5, says that a file kind says what it asserts, that the review summary gains one item for each with its file kind, its surface, and a fixed sentence, and that the publish page shows them with the sign-ins, before the tools. It does not say what type a field's kind is in a crate that cannot see the provider contract's shape, when the compiler asks, in what order the items come, or where on the page they go now that [decision 0418](0418-the-publish-page-lists-the-surfaces-first-and-marks-each-new-address-where-it-lists-it.md) put the surfaces first.

## Decision

1. **The type.** `AssertedRead`, in `a2m-files`, is the entry's name, its method, its path as the entry writes it, and its body's fields, each an `AssertedField`: its name, the word of its shape's `type` (`string`, `integer`, and so on), and the texts of its `enum`, empty when any text fits. The kind is that word, as text, because `a2m-files` sits below the provider contract and cannot name its `Kind`. A request with no body and one whose body is an object with no fields both give no field: the page says "It sends no field" of either, which is true of both.
2. **When the compiler asks.** `FileKind::asserts(&self, doc)`, empty by default, is asked of every document of a pack's file kind that passed its schema and stage 3, beside its names, with the surface it lives on. The summary's `asserted` holds one `AssertedReview` for each read given: the file kind, the surface, the read, and `says`, the fixed sentence `ASSERTED_NOTICE`: "the person's word is the only evidence that this request changes nothing". The items are ordered by surface, then file kind, then entry. `map_text` passes the entry, the method, the path, and each field's name, kind, and texts through the function, as for every other text of the summary.
3. **Each kind answers by its own rule.** A page request answers when it is marked `effect: read`, its method is not `GET`, and it says `read_asserted: true`; an endpoint when it is marked `read`, HTTP does not mark its method safe, and it says `read_asserted: true`. Each reads its body's fields with the same reading its compile uses. An entry whose body cannot be read is named with no field; its compile refuses it, so no version is published from it.
4. **Where on the page.** The publish page lists where each surface leads first (decision 0418); then "How it signs in"; then "The reads a person asserted", which says that each request is sent by a method that may change the system and that the browser lets it through for every read of its surface, whichever tool sends it; then "What each tool may do"; then the kernel's lines and the checks' warnings. Each asserted read shows its method, its path, its surface, the file kind and the entry that assert it, the sentence in bold, and its fields with their kinds and their `enum`. An act with no asserted read shows no such heading. Every text is escaped as every other text of the page is.

## Alternatives

| Alternative | Why not |
| --- | --- |
| A field's kind as the provider contract's `Kind` | `a2m-files` is below the contract in the layering table; a second enum of the same words in `a2m-files` would be a third statement of the shape's words |
| Build the items from the compiled pack sections | The kernel never reads a pack's payload, and the HTTP pack's compiled endpoint keeps no field list for a read |
| The asserted reads in a section of their own before the sign-ins | Decision 0394 put them with the sign-ins, before the tools; both are statements about a surface that hold for every tool on it, and a credential's reach is still the first thing to read after where the surfaces lead |
| One item per tool that reaches an asserted read | The tab lets the request through for every read of the surface, so the statement is the surface's, not a tool's (decision 0394, alternatives) |

## Proof

- `crates/a2m-files/src/tests/review.rs`, `each_asserted_read_is_one_item_with_its_request_its_fields_and_the_sentence`: a target with one asserted page request and one asserted endpoint gives two items, each with its method, path, field names, kinds, `enum`, and the sentence; every text passes `map_text`; a target with none gives none.
- `crates/a2m-pack-http/tests/compile.rs`: an endpoint marked `read` by `POST` with `read_asserted: true` is one item; the target as written gives none.
- `crates/a2m-local/src/tests/surface.rs`, `the_act_page_shows_sign_ins_before_tools_and_escapes_and_hides_every_secret_value`: the heading after the sign-ins and before the tools, the request escaped, the sentence in bold, the field with its `enum`.
- Through the product, `crates/anything2mcp/tests/asserted_publish.rs`: the publish page of a version asserting `POST /api/cases/rows/search` names it after "Where it leads" and before the tools; the person carries the publish out, and the read returns the two open rows the test application holds, live, which heard one search and never an archive.

## What would reopen it

A pack whose asserted request cannot be told by method and path alone, such as one keyed by a long text of its body; or a page where the asserted reads are many enough that they push the tools out of reach.
