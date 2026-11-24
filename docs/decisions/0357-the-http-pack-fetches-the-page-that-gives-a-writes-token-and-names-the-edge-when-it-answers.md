# 0357. The HTTP pack fetches the page that gives a write's token, and names the edge when it answers

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** the `http` mode of [the HTTP pack](../architecture/crates/a2m-pack-http.md) (a step kind `token`, the endpoint field `anti_forgery`, two failure codes of the edge and three of the token), [spec section 303](../spec/3-packs/303-the-http-pack.md), the `anti_forgery` row of the [file formats](../reference/file-formats.md#endpointsyaml), the `token` row of [steps, snapshots, and diffs](../reference/steps-snapshots-and-diffs.md), the [test application plan](../plan/test-application.md), and package 4.1 of the [roadmap](../plan/roadmap.md). Builds on decisions [0139](0139-a-write-that-was-sent-is-never-settled-by-an-answer-that-is-not-the-applications-verdict.md), [0206](0206-the-page-http-mode-writes-by-put-with-the-pages-own-token-in-an-acting-tab.md), and [0270](0270-the-http-pack-sends-an-asserted-read-only-from-a-published-version-and-bounds-what-it-leaves-running.md). Changes nothing in the kernel.

## Context

Package 4.1 listed two things the `http` mode did not do, both named in section 303 and both with a case in the test application: a write whose application checks an anti-forgery token that its page gives anew on every load (case 5), and an answer that comes from the edge in front of a system and not from the system (case 23). An endpoint file that wrote `anti_forgery` was refused as `not_built`, and a 429, or a 403 page, stopped as `unexpected_status`, which says only that the endpoint did not declare the status.

## Decision

1. **A write that needs a page's token has a `token` step before its `request`.** The endpoint says `anti_forgery: {page, from, header | field}`: the path of the page on the endpoint's own surface (its placeholders are the endpoint's own), the one place on that page that holds the token (`from: {meta: name}`, `{input: name}` for a hidden input, or `{cookie: name}` for a cookie the page's answer sets), and the one place the write carries it, a `header` or a body `field`. `anti_forgery: none` says the endpoint checks no token. This is the form `page_http` reads in `page_requests/` ([decision 0206](0206-the-page-http-mode-writes-by-put-with-the-pages-own-token-in-an-acting-tab.md)), with `page` added, so one file can be read by both; a token taken from another endpoint's answer, which section 303 names, is not built.
2. **The page is fetched in the same pass as the write, by a GET that writes nothing.** The lowering puts a `token` step first in the write's `act` list. It is not a commit point, and it takes the `path.*` values the write's own path takes. It sends where the endpoint's requests go, with the endpoint's credential, and follows no redirect.
3. **The token is read from the page as data, never run.** Tags are taken apart one at a time, comments and the text of `script`, `style`, `textarea`, and `title` are skipped, and exactly one text must be held at the place the endpoint names. Two different texts, or one that cannot be sent in a header, stop as `anti_forgery_unreadable`; none, or a page that answered with another status, as `anti_forgery_not_found`. Both stop before anything of the write is sent, so both are `not sent`.
4. **A token is sent once.** The driver holds it under the endpoint and the record's key, takes it into the write, and drops it when a request may have reached the system, as it does a version. A `request` step found with no token held is refused as `token_not_read`, class `stale`, nothing sent. So an engine that retries the write step alone, or a resume after a lost answer, never sends a spent token; the engine's retry of the whole list fetches a new page.
5. **The token is a secret by use.** It travels in a header marked sensitive, which no trace records by value, and its header is listed by name only in the recorded exchange. When the answer to the write quotes the token in any letter case, which an application that echoes a refused body does, the system's words are not quoted back, and no value taken from the answer may hold it (`answer_holds_secret`). Nothing is added to the redactor, which the driver cannot reach.
6. **An answer the endpoint does not declare, from the edge, has its own names.** After the endpoint's own declared answers are tried, status 429 stops as `rate_limited_by_edge` (with the seconds the answer asks to wait, as a number), and status 403 with a page (`text/html` or `application/xhtml+xml`) as `forbidden_by_edge`, both of the class `edge_blocked`. The page's words are never quoted, and nothing is retried. A 403 that is not a page, such as JSON, stays `unexpected_status`: it is the application's own. An endpoint that declares 429 or 403 keeps its meaning. A write that was sent and answered so is in doubt as before, because the class is not one the ledger takes as settled. A challenge page (`challenge_page`) is not built: it has no status or header that can be told from a page without naming a vendor.

## What 4.1 now proves, case by case

Package 4.1's proof is that the test application's cases pass over direct requests. Each case that is the HTTP pack's, and where it is proven:

| Case | Proven by |
| --- | --- |
| 1 to 4 | `crates/a2m-pack-http/tests/driver.rs`: reads, writes with read-back, refusals, a stale version, a slow and a failing endpoint |
| 5 | `crates/a2m-pack-http/tests/forgery.rs` and `crates/anything2mcp/tests/forgery_write.rs`: through the driver, and through the product's engine loop with approval, a lost answer, and a scan of everything the product keeps for the token |
| 16 | `crates/anything2mcp/tests/normalized_write.rs`, over the `http` mode ([package 3.4](../plan/roadmap.md)) |
| 17, 20 | `a_write_whose_answer_is_lost_is_a_possible_commit_and_is_not_sent_again`, and `crates/anything2mcp/tests/verify_after_loss.rs`: a write is sent once whatever its answer |
| 21 | `an_approval_made_stale_at_the_stand_writes_nothing_and_a_superseded_diff_is_refused` in `crates/anything2mcp/tests/faults.rs` |
| 23 | `crates/a2m-pack-http/tests/edges.rs`: a page, a limit, a declared answer, and a 403 that is not a page; the application is asked how often it was reached |
| 29, 30, 31, 34, 35, 36 | the tests of credentials, of the description, of drift, of a redirect, and of the drafter, as before |

Cases 18 and 19 (a created record's identifier, and a header with its lines) are the engine's, package 3.8, and the test application does not serve them yet; 22 is the compile-time refusal `http.signed_endpoint`; 24 needs downloads, which the pack does not build. None of them is a step of this pack's `http` mode that is missing.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Fetch the token inside the `request` step | One step would send a GET and then a write, so a write step would carry two requests, and a trace could not tell where a failure was. A separate step shows in the plan, can fail alone as `not sent`, and is what section 303 already said |
| Take the token from a browser page, as `page_http` does | That needs a browser for a write that the application accepts over plain requests. This is the HTTP pack's point |
| Hold the token for the whole pass and send it again on a retry | An application that takes a token once refuses the retry, and the refusal would look like a forged write. A fresh page is the only honest second try |
| Register the token with the redactor | The driver is handed a read-only redactor, by design: only the vault registers secrets. The driver itself withholds what quotes the token |
| Name every 403 `forbidden_by_edge` | An application's own refusal of a credential is a 403 too. A page, where data was asked for, is the sign an edge gives; the rest is left to the endpoint to declare |

## What this risks

A token placed by a page's script after load, or one the page holds in a form that is not a plain `meta`, hidden `input`, or cookie, is not found: the write stops as `anti_forgery_not_found` before anything is sent, which is the safe side. An application whose own 403 is a page is named an edge's: the message says what was seen, and a declared `refused` answer for that status keeps it the endpoint's. A page that gives its token through a header of its answer is not read.
