# 0190. The `page_http` mode reads from inside the page, in an observing tab

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the in-page read of [`a2m-pack-web-browser`](../architecture/crates/a2m-pack-web-browser.md), request matching in the `web_capture` discoverer of [`a2m-pack-web-probe`](../architecture/crates/a2m-pack-web-probe.md), the `page_http` mode, the `page_requests/` file kind, and two drafting rules of [`a2m-pack-web-ui`](../architecture/crates/a2m-pack-web-ui.md), and sentences of spec sections [301](../spec/3-packs/301-the-web-pack.md), [201](../spec/2-kernel/201-archetypes-and-modes.md), and [402](../spec/4-building-tools/402-the-scout-and-who-does-what.md). Takes up what [decision 0187](0187-the-web-pack-matches-a-screen-by-its-address-and-reads-by-key.md) left of a read over a request the page made, under the rules [decision 0144](0144-the-web-drivers-and-discoverers-after-their-safety-audit.md) set for a request made from inside a page. Roadmap package 4.2, its read half.

## The plan, as written before building

1. The browser engine gains a read from inside an observing tab that takes no method and sends GET alone, to a path made from a pattern and values each one plain segment, with no header a caller chose and no body, following no redirect, read as text up to a limit, with the tab's guard still under it.
2. `page_http` is registered in the web UI pack's manifest, as 301 and the crate's design place it, for reads only. Its lowering makes `goto` (the page that makes the request) and `request` (make it, check it, extract); a write is refused by name.
3. The probe joins the requests a page made by itself into one read by key, as it joins pages into one screen; a rule drafts the entry and a read over it.
4. Proof against the page of an order's totals; 5. both reads of one order measured.

*Alternatives weighed.* For where the entries live: the HTTP pack's `endpoints/`, which it already registers for `web` surfaces and which a lowering may read by declaring it. Reading would have worked; drafting would not: the drafter lets a rule draft only a kind its own pack owns (`may_draft`), and a test of this crate cannot register the HTTP pack. For the wait: a `wait` on a screen's `ready`. A drafted screen needs a `ready` the exchange does not give, so the drafts would not compile. For check and extract: `validate` and `extract` as steps, as 301 listed them. The answer would then be held between steps, unchecked, in the driver's state.

## Decision

| Question | Decided | Alternative weighed |
| --- | --- | --- |
| The engine's call | `ObserveTab::read_from_page(&PagePath, deadline) -> PageAnswer { status, media_type, body }`. `PagePath::fill(pattern, values)` refuses a value that is not one plain segment (`input`, `address_value_refused`, new in the engine's codes) and a pattern with an empty segment, a query, or a fragment (`address_invalid`); `plain_segment` is the one rule, which the `ui` driver's `goto` now uses too. The helper makes the address from the page's own origin, refuses a page with none (a `data:` page), checks the page's origin against the tab's in the same call, fetches with GET, `credentials: same-origin`, `redirect: manual`, `cache: no-store`, no header and no body, and stops reading past the limit. No header of the answer is returned | Opening `ActTab::fetch` to observing tabs with a check on its method. Its method, headers, and body are the caller's; a call that cannot name a method cannot send another |
| The guard beneath it | Only an observing tab has the call, so the browser still fails every request of the page that could write. Shown: the same request, from the same page, with POST, is failed in the browser and the application sees no POST | Relying on the call's own GET |
| Where the mode lives | In the web UI pack's manifest: `page_http`, archetype `request`, rank 40, verbs `read`, steps `goto` and `request`, no commit steps; its own `PageHttpDriver` over the same `Browsers` | A pack of its own. 301 and the crate design both put it here; they agreed |
| What the tab is | An observing tab in every posture, in the session's profile: the run may only read | An acting tab for a live run, as `ui` has. Nothing a read does needs one |
| The named condition | The page's load: `goto` returns when the browser says the document loaded, which is the condition the request waits on. The page has the session then; its own script need not have run | A screen's `ready`. Needed for the write half, where the page must hold its token first |
| Entries | `page_requests/{name}.yaml`, a file kind the web pack owns for `web` surfaces, written as a part of an endpoint entry: `endpoint`, `surface`, `method: GET`, `path`, `effect: read`, `response_schema` (`type: object`, `properties` with a `type` each, `required`, `additionalProperties`), and one field the catalog has no word for, `page`, the page that makes it | `endpoints/`, the HTTP pack's: one catalog, as rule 8 asks. See "One catalog later" |
| Check and extract | Inside `request`: a status other than 200 is `unknown_variant`, `unexpected_status`; another media type than JSON, a body that is not an object, a required name missing, or a declared name of another kind is `drift`, `response_schema_mismatch`, as section 301 and the HTTP pack class a changed answer; a name the entry does not declare is ignored unless it says `additionalProperties: false`, the catalog's rule. No value leaves before the check | `unknown_variant` for every shape that differs. 301 and the HTTP pack already say `drift`; one meaning across modes |
| What is read | A string as text, an integer as a whole number, a boolean as a truth value. The compiler refuses an output on a number with a fraction, a nested value, an undeclared name, or a name the answer may leave out | Reading a number with a fraction as a decimal. The JSON reader of this crate parses it to a float first |
| Writes | The lowering serves `read` alone, so a write verb is the compiler's `kernel.unknown_verb`; a `read` on an operation declared to write is `web.page_http_writes` | Declaring the write verbs in the manifest only to refuse them: a manifest would then say the mode writes |
| Request matching | In `web_capture`: requests a page made by itself, of one method and origin, answered with one status and one JSON shape, whose paths differ in one segment, made by pages whose paths differ in one segment holding the same value, are one `exchange` finding at the place of the first, its `path` and `made_by` with the placeholder (named as screen matching names one), and `paths`; the requests' own are not given. Endpoint inference still sees every request | Matching in the drafter, which sees one finding at a time |
| Drafting | From an `exchange` with `made_by`, GET, JSON, status 200, on exactly one `web` surface: `page_requests/{words}.yaml` and `operations/{words}_request.yaml`, `{target}_{words}_get`, `{words}` the request path's plain words. Inputs and key are the placeholders; outputs the plain top-level values seen in every answer, `untrusted_text`, `integer`, or `boolean`; nested values and numbers with a fraction left out; no example | Naming from the page's path. A screen's reads are named from the page's path, so the request's would collide; a page answered a document and the request JSON, so no one path gives both in one discovery |

## The write half, and what it needs

- The anti-forgery token the page holds, sent without being read out of the page: a token the helper places from the page's own cookie or meta into the header inside the same in-page call, and a header named by the entry (`anti_forgery`), never by a caller.
- A commit point the `request` step marks, a tab that may write for that one request, and the ledger's intent before it is sent; and the HTTP pack's rules for what was sent and not answered (decisions 0139 and 0154).
- A read-back endpoint that tells the new state from the old.

## One catalog later

The entries are in the endpoint catalog's form, so they move to `endpoints/` unchanged but for `page`. Either of two changes lets them share one catalog: the drafter lets a rule draft a kind its pack's lowering declares it reads (a kernel change), or the HTTP pack drafts an `endpoints/` entry from an `exchange` that names `made_by` and adds `page` to its schema; the `page_http` lowering then declares `(endpoint, web)` under `reads` and reads `cx.entries`. Until then a hand-written `page_http` entry goes under `page_requests/`.

## Measured

Chrome for Testing, headless, on Linux, the order `A-100`, through the registry's driver handle in a run that may only look, ten runs after one warm run, each from preparing the tab to its release; the `ui` read is a hand-written screen of the totals page that reads the total the page shows once its own request answered. A first measurement, not a budget.

| | Whole run, median (range) | Steps after the tab opened, median | The last step alone, median |
| --- | --- | --- | --- |
| `ui`, the stand's own delay of 200 ms on the totals | 292 ms (282 to 297) | 237 ms | 3.4 ms (`read`) |
| `page_http`, the same | 307 ms (279 to 314) | 244 ms | 208 ms (`request`) |
| `ui`, no delay | 118 ms (85 to 141) | 49 ms | 3.1 ms |
| `page_http`, no delay | 99 ms (86 to 119) | 46 ms | 9.1 ms (6.4 to 11.6) |

The two results agree where they overlap: the page shows the total quantity as text, and the request gives it as the same whole number, which is what the control interface holds. The mode is not faster per run here, because both reads go to the same page and wait for it to load, and this page's own read is the one `ui` waits for. What `page_http` saves is the screen: a read of a value no page shows, and, once a run keeps a page open between reads, about 9 ms for each read after the first. The exit test's tenfold is for shadow over many records (package 4.5), not this.

## Consequences

- `two_pages_that_ask_for_one_read_by_key_give_one_exchange_with_a_placeholder` (probe): discovery of the totals pages of `A-100` and `A-101` gives one `exchange` at `/api/orders/{order_id}/totals`, made by `/orders/{order_id}/totals`, the same twice, kept in `a2m-pack-web-ui/tests/fixtures/findings/order_totals_exchange.json`.
- `a_request_the_page_made_drafts_a_page_http_read_that_reads_each_order_by_key` (web UI): the entry and `example_api_orders_totals_get` are drafted beside the order screen's two reads, the same bytes twice, and compile; the read returns the lines, total quantity, and total length the control interface reports for both orders; six keys that leave a segment stop as `address_value_refused`, not sent, and the application saw nothing; a renamed field and a field of another kind stop as `response_schema_mismatch`, a 503 as `unexpected_status`; nothing is written; the page's cookie and token are in no result, finding, draft, capture, or log.
- `an_observing_tab_reads_from_inside_its_page_with_get_alone_and_its_guard_still_holds` (engine): two orders by key, one GET each and nothing else; refused values send nothing; a redirect is not followed; POST from the same page is failed; a page with no origin reads nothing. `a_read_from_inside_the_page_longer_than_the_limit_is_refused_whole`.
- `nothing_is_drafted_for_a_request_that_could_write_is_not_json_or_went_elsewhere`; `a_page_http_binding_on_an_operation_that_writes_is_refused_by_name`.
- A read that needs the anti-forgery header the page sends cannot be made yet: it is answered with the application's refusal and stops as `unexpected_status`.
- The query the page sent (`view`, `lang`) is not sent: its values were never kept. An application that needs one answers otherwise, and the read stops.
- A matched exchange keeps the first request's limitation "seen once"; the sampled limitation beside it says it was matched.
