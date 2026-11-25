# 0999. A watching tab hands out each screen with its document, and a request goes to the screen of its own document

**Status:** Accepted. Design decision, made by the worker who found an intermittent failure of the capture test, under the lead's brief. The owner had not read this record when it was merged. It changes no public contract: `CapturedRequest` and `TrafficCapture` are as they were; the web pack's base gains `WatchTab::next_screen`, `WatchTab::exchanges_through`, `WatchedScreen`, and `DocumentMark`.

**Date:** 2026-11-25

**Changes:** [`a2m-pack-web-browser`](../architecture/crates/a2m-pack-web-browser.md), the watching tab (`tab/watching.rs`); [`a2m-pack-web-probe`](../architecture/crates/a2m-pack-web-probe.md), `web_use` (`watch.rs`); the test application, whose order summary can show a slow picture. It supersedes point 5 of [decision 0411](0411-the-watching-tab-records-from-the-browsers-network-events-and-keeps-each-screen-without-its-values.md) where it says what a screen waits for, and point 3 of [decision 0442](0442-a-persons-use-is-watched-by-web-use-and-what-it-saw-twice-is-drafted-held.md), whose last sentence named this as the better design.

## Context

Once in CI, on a slow macOS runner, the capture test drafted no held read of the order totals, though the application heard both summaries' requests. The cause, shown in a trace of the watching tab: a screen's snapshot waited for the browser to say the document was idle on the network (`networkIdle`), which on the test's summary page comes about two seconds after it loads, while the test's person moves on about three seconds after it loads. On a slow machine the person left first. The tab then took the snapshot of whatever page the frame showed when the wait ended, the next document's or one after it, and `web_use`, which put every request to the screen of the last snapshot, put the summary's own request, and the next page's, to that screen. The two totals requests were then made by two different pages, were never matched into one read by key, and each was seen once.

A person who reads a page for a second before moving on does the same to every machine, so the fault was the product's, not the test's.

## Decision

1. **A screen is one address of one document.** The watching tab keeps the loader of each document of its own frame in the order the documents are parsed, and the address the browser said each one is at: its document's request, redirects followed, and each move within it. A screen is looked at in the last document parsed, at that address; a document the browser named no address for is no screen. One document loaded again at one address is another visit.
2. **A screen waits for its own document alone.** The tab waits until the browser says that document is idle on the network and no recorded request awaits its answer, or `SETTLE_BOUND`; and it stops waiting at once when another document of the frame is parsed. A snapshot is kept only when the same document is still the last parsed after it is taken and its address is the screen's. Otherwise no snapshot is kept, and the screen is still handed out, missed, with its address and its document.
3. **Each screen is handed out once, with its document.** `WatchTab::next_screen` gives the oldest screen not yet handed out, as a `WatchedScreen`: its address, its `DocumentMark`, its snapshot or none, and whether it was past `SCREENS_KEPT`. `WatchTab::screens` gives what it gave before, from the same screens.
4. **A take can stop at a document.** `WatchTab::exchanges_through(mark, deadline)` takes, and waits for, only the requests sent under the documents of the frame up to that mark; a request of a document parsed after it, or not parsed yet, stays for a later take. A request the browser named no document for is taken with any. With no mark it takes everything, as `exchanges` does.
5. **`web_use` puts a request to the screen of its document.** For each screen the tab hands out, oldest first, it takes what the screen before sent since its own take, put to that screen, then the requests up to the new screen's document, put to it; a missed screen is put to by its address. At the end it takes everything left, put to the last screen. A request is still read only through `take_capture`, by a traffic capture of the contract over the tab and a mark.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Lengthen the test person's stay on each page | The test would pass and a person who reads a page for a second would still have its requests put to another page |
| Wait for the page's own requests and the `load` event, and no longer for `networkIdle` | A page that asks after it loads would be snapshotted before its request; a quiet window of the engine's own would be a fixed wait |
| Add the sending document's address to `CapturedRequest` | A change to a public contract that `a2m-provider` holds; the base can say it through its own interface, and the contract's check still applies to every request |
| Keep the address read from the page in the snapshot's own call | Measured: the page answers from the next document before the browser says it has been parsed, so an address read that way could be put to the document before it |

## Proof

`a_person_s_use_is_captured_in_a_watching_tab_and_drafted_held` (`crates/anything2mcp/tests/capture_use.rs`): the second order summary the person visits shows a picture the test application answers only after eight seconds, so its page is never idle while the person reads it. Without this decision the test fails every time, with no held page request for the totals, as the CI run did; with it, the summary's screen is missed and its request is put to it, and the read is drafted held. The watching tab's own test, `a_watching_tab_records_every_method_by_structure_and_the_engine_sends_nothing`, still gets a snapshot of every screen in order.

## Revisit if

A page moves within its document so often that its requests are put to a screen they were not sent from; then a request is stamped with the address of its document when it is sent.
