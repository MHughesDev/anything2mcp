# 0399. The `page_http` write stops at the edge and at an ended session before it sends, proven through the product

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** the `goto` of the `page_http` driver of [`a2m-pack-web-ui`](../architecture/crates/a2m-pack-web-ui.md); the test application's case 5, which gains a copy behind its session (`targets/example-app/src/forgery.rs`); a sentence of spec section [301](../spec/3-packs/301-the-web-pack.md). Closes roadmap package 4.2 with the fault rows of a write that [decision 0206](0206-the-page-http-mode-writes-by-put-with-the-pages-own-token-in-an-acting-tab.md) left: a blocked surface, and an ended session told by the target's own sign, through the engine loop of the product.

## The plan, as written before building

1. Every fault of the definition of done for a write, through the product, against the note of case 5: input that would send the request elsewhere; a surface an edge blocks, before the write and during its read-back; a session that ends between the approval and the resume. The connection lost after the write was sent is already proven through the product (`verified_write.rs`).
2. Whatever those show the driver lacks, built in the driver.

*Alternative weighed.* **Leave the edge to the in-page request.** Before this record, a `goto` that the edge answered with its own page went on: the page held no token, so the write stopped as `drift`, `anti_forgery_not_found`, unsent, which is safe and names the wrong cause; and a read-back's `goto` went on to a request the edge answered too, `unexpected_status`. The `ui` driver already names the edge from the page's status (decision 0357's codes); one rule for one surface is better than two.

## Decision

| Question | Decided | Alternative weighed |
| --- | --- | --- |
| The edge | `goto` of `page_http` classes the page as the `ui` driver does (`edge_refusal`, now shared in the crate): a challenge, 403, 429, or 503 stops the step as `edge_blocked` with that code, answered, before anything is sent from the page. It is never retried and a challenge is never answered (rule 5) | The token's absence, as before |
| Holding the surface off | Not done in `page_http`: the `ui` driver's memory of an edge's refusal is its own. A `page_http` stop is not retried by the engine, so no storm follows from one run; a second run asks the edge again | One memory for both drivers. Its sixty seconds would also refuse the reconcile that settles a write left in doubt, until it passes |
| The ended session | Proven through the product with `form_login`: the resume's first page, that of the read before the write, answered by the declared sign-in screen, stops as `session_expired`, `needs_login`; the run rests at `needs_login`, no intent is recorded, no token is used, nothing is stored; once signed in again, the resume writes once and reads back | |
| The stand | Case 5 is also served behind the stand's session: the page at `/secure/orders/{order_id}/note`, under the page guard that answers with the sign-in page, and the interface at `/secure/api/orders/{order_id}/note`, under the session guard that answers 401; the same notes | A session-bound token: not needed for this proof |

## Proof

`crates/anything2mcp/tests/page_http_faults.rs`, each asking the test application what it stored:

- `input_that_would_send_the_request_elsewhere_reaches_nothing_at_the_stand`: an undeclared field, keys that read as a path, an address, a host, or a header, and a note of another kind are each `input`, `rejected`; the application heard nothing, and no approval was asked.
- `a_surface_an_edge_blocks_writes_nothing_and_a_block_during_the_read_back_leaves_doubt`: every read diverted to the stand's edge, the resume stops as `edge_blocked`, `forbidden_by_edge`, nothing written and no token used; the edge after the write, the record is stored once; once the edge lets reads through, the resume reconciles and sends nothing again. The test expects the stop during the read-back to be `possible_commit`, `no_confirmation`, which the engine is being changed to give for any stop during a read-back after a sent write; on the engine as it stands it is `edge_blocked`, status `stopped`, and the rest of the test passes.
- `a_session_that_ends_between_the_approval_and_the_resume_sends_nothing_from_the_sign_in_page`: as in the table.

## Not done

- POST, PATCH, DELETE; `create`, `upsert`, `delete`, `submit`; `path_params` and `query`; a read that needs the anti-forgery header: as decision 0206 left them.
- The keeper did not renew the ended `form_login` session by itself in this test: the run rested for the person. Renewal through `page_http` is the keeper's, and is not what this package proves.
