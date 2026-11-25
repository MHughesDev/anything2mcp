# 0477. A read goes on past what an observing tab refused when the page made it, and the run says so

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-25

**Changes:** what [decision 0111](0111-an-observing-tabs-lock-as-built-and-what-the-browser-was-measured-to-do.md) and [decision 0292](0292-a-web-tab-sends-only-to-its-surface-and-the-destinations-its-tool-declares.md), point 3, make of a refusal in an observing tab: the drivers of `a2m-pack-web-ui` (`Limits` and `refusal` in `src/driver.rs`, and `src/page_http.rs`), the base's report of such a refusal (`RequestShape::refused_when_made` and `REFUSED_WHEN_MADE` in `a2m-pack-web-browser`), the test application's `/leaving/needs-shared/...`, the designs of both crates, [section 301](../spec/3-packs/301-the-web-pack.md), and `SCOPE.md`. The lock itself does not change: an observing tab still refuses every socket, peer connection, shared worker, and service worker its page makes, and sends nothing through it. No kernel crate, driver interface, provider manifest field, or file format changes, and no failure code is added; the meaning of `page_write_refused` gains the acting tab's case.

## Context

Field test 11 met a forge whose every signed-in page opens a shared worker as it loads (finding 63). The observing tab refused it when the page made it, so nothing was sent; the page went on without it and showed its record. The read stopped all the same, as `policy`, `page_write_refused`, because the drivers turned every refusal of the tab into a stop at the step's check of what the tab refused. Decision 0292, point 3, says why for a request outside the reach: the driver cannot know what the page does without it (rule 7). The words and the address of that stop were fixed with the finding; whether it should stop at all was left to this record, and H-54 bets that many signed-in pages are like the forge's.

What the refusal leaves open is not where anything was sent: nothing was. It is whether the page shows its record truly without what it made. A request the page sent and the tab failed is different: the page asked the system for something and did not get it, and a request outside the reach is a place the tool would send to. A socket or a worker refused when made sent nothing anywhere, in either case.

## Alternatives

| | What a person building over a real web application pays | What could go wrong in a read | Rules served |
| --- | --- | --- | --- |
| **(a) A limitation in the observing tab: the read goes on where the screen's `ready` holds, the run says what was refused, and a page that shows the record absent after such a refusal stops** | Nothing: a page that goes on without what it made is read | A page that fills in its record through what it made, later than its `ready` holds, is read as it shows before that. A page that says the record is absent because it went without is a stop, never an absent record | Rule 2 (nothing is sent; the record shown is what the screen declares), rule 7 for the absent case, the North Star |
| **(b) A declared allowance: the surface names what its pages may make and go on without, and the read goes on only for that** | A run that stops once per application, and a line in the surface file, which the drafting rules could fill in | The same as (a) once declared: the declaration says nothing about whether the record is shown truly | Rule 7 in form; it guards nothing, since nothing was sent either way, and it costs a person a step for every signed-in application like the forge |
| **(c) Let the worker run in the observing tab, held by the interception** | Nothing | A shared worker's requests are not the page's, and whether one made past the lock would be attached and held was never measured (decision 0111). A socket is not seen by the interception at all | Fails rule 2: what it sends is not held to what the tab may reach |
| **(d) As before: every such refusal is a stop** | Every signed-in page of the forge, and any page like it, cannot be read | Nothing | Safe, and fails the North Star for every page H-54 counts |

**Chosen: (a).** (b) would ask a person to vouch for something whose risk the declaration does not touch: what makes a read of such a page untrue is the page, not where it sends, and the screen's `ready` and its variants are already the person's statement of what the record looks like when it is shown. (c) needs a lock the browser was not found to give. (d) is the state this record leaves.

## Decision

1. **A refusal made when the page made it is kept, not stopped on, in an observing tab.** A WebSocket, a WebTransport session, a peer connection, a shared worker, and a service worker that the observing tab's lock refused when its page made it (`REFUSED_WHEN_MADE`, asked by `RequestShape::refused_when_made`) are kept by the driver as a limitation of the run (`Limits` in `src/driver.rs`), in both the `ui` and the `page_http` drivers, in every posture. Every other refusal of the observing tab stops as before: a request it failed, a request outside the reach, a move, a window, a download, a worker or frame that could not be locked (`WORKER`), and a word of the page's lock that no rule knows (`REFUSED`) (rule 7).
2. **The acting tab's are still a stop.** A socket outside the reach, a peer connection, a shared worker, or a service worker the acting tab refused stops the run as `page_write_refused`, now worded for the tab a write is made in: a write is not made on a page that goes on without what it made, since what the page then does, after the commit point too, is not known.
3. **The read goes on where the page goes on.** The `ui` read reads the screen once its `ready` holds, as every read does; nothing is waited for beyond it. In `page_http` the read is the driver's own request from inside the page, so what the page made does not change what it reads.
4. **The run says what was refused.** Each refusal kept is told once, on the outcome of the step during or before which the tab heard of it, as a `refused` detail of what the step measured in its capture of how it was done: the base's word and where it led, its origin and its path by structure (`refused=SHAREDWORKER http://host:port/escapes/shared-worker.js`). Two runs of the same page may hear it at different steps, so it is not in the trace's signature.
5. **A page that needs what it made still stops.** In the `ui` driver, after a refusal kept: a page that shows a variant of handling `absent` stops as `unknown_variant`, `unexpected_page`, saying that whether the record is absent is not known, since the page may say so only because it went without; and a step that stops as `unexpected_page`, `locator_not_found`, or `timeout`, with nothing sent, keeps its own class and code and says, in its message and its next step, what was refused before it and that a page that needs it cannot be read by the web pack. Every other stop is the step's own.

## What the test showed

The test application's `/leaving/shared/orders/A-100/items/L-101` opens a shared worker by a relative address as it loads and goes on without it; read through the `ui` driver, it gives the stored description and quantity, the run's measured details name `SHAREDWORKER` once with its address, the signature does not, and the trap that counts what the worker would send counts nothing. `/leaving/needs-shared/...` says the record is not found when its worker is refused; the read stops as `unexpected_page`, names the shared worker and its address, sends nothing, and the record is there in the store. What it showed about H-54: the mechanism the bet needs holds on a stand page, and the risk it carries is narrower than a stop assumed, since nothing is sent; whether many real pages show their records with the worker refused is the count H-54 still asks for.

## Not done

- **A record filled in late.** A page that shows a placeholder record once its `ready` holds and fills in the true one through what it made is read as the placeholder. A screen whose `ready` names a value-bearing element guards against this; nothing else does. H-54's count is to say how often it is met.
- **The scout.** A scout's observing tab already reports what it refused as a count by word and goes on; it carries no word of this record into the `screen` finding.
- **A refusal heard after the last step.** A refusal the tab hears only after the run's last step ended is not told. The base says a refusal made before a `goto` or a `wait` that followed it is heard by then.

## Revisit if

- H-54's count finds pages that show a record with the worker refused that differs from the record shown with it.
- The browser gains a way to hold a shared worker's or a socket's sends in the observing tab, which would let (c) be measured.
