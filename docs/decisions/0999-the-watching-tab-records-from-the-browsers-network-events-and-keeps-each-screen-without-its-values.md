# 0999. The watching tab records from the browser's network events, and keeps each screen without its values

**Status:** Accepted. Design decision, made by the worker who built package E2c, under the lead's brief. The owner had not read this record when it was merged. It adds to a public interface of [`a2m-pack-web-browser`](../architecture/crates/a2m-pack-web-browser.md), the `Observer`, and to the provider contract's `CapturedRequest`, as [decision 0392](0392-a-page-that-reads-by-a-method-that-may-write-is-learned-from-a-persons-watched-use-and-read-through-an-asserted-request.md), point 2, says it must.

**Date:** 2026-11-24

**Changes:** `Observer::watching_tab`, `WatchStart`, `WatchTab`, `WatchedScreens`, and `TrafficCapture for WatchTab` in `a2m-pack-web-browser`; `PageExchange` and `CapturedRequest` gain `body_media_type`, `body_fields`, and `body_names_withheld`; the code `address_looks_like_an_action` is declared in the base too. It builds point 1 of decision 0392 and settles what that point left to the builder.

## Context

Decision 0392, point 1, says what a watching tab is: a tab of the window package 2.3 built, where the engine sends nothing, nothing is failed for its method, the window is held to the surface's origins, every request is recorded by structure with its body's names and kinds, and a snapshot is taken of each screen the person visits. It does not say where the base hears a request it holds no interception for, which requests of a page count, what of a screen is kept, how a request is closed that the browser never says more of, or where the check of the first address lives. Each is a choice between real alternatives.

What the browser does was measured on Chrome for Testing 153 on Linux, in the row's own test: `Network.requestWillBeSent` carries a script's body whole in `postDataEntries` with the bound of `Network.enable` set, for JSON and for a form; a `fetch` whose page never reads the answer gets its answer's head (`Network.responseReceived`) and then no `loadingFinished` and no `loadingFailed`, also after the page has moved to another document.

## Decision

1. **A request is heard from the browser's network events, not from an interception.** The tab holds only documents and what a page loads with no body, to its origins, as a sign-in tab does; a script's request passes untouched, and is recorded from `Network.requestWillBeSent`. So nothing the person does waits on the engine, and no request can be failed for its method by a fault of the engine's.
2. **What counts as the page's request:** a script's (`XHR`, `Fetch`), a beacon's (`Ping`), and a document's that carries a body, a form the person posts; each to one of the tab's origins. A document's `GET` is a screen, which the snapshot covers. Workers and frames are not attached to, so their requests are not recorded; that is a limit of what is seen, never a request let through unseen, since nothing in the tab is failed for its method anyway.
3. **A body is read up to 64 KiB** (`LONGEST_WATCHED_BODY`), from its parts as decision 0407 reads them. JSON gives its names and kinds by the rule of an answer's (`fields_of`); a form gives its entries' names, each a `string`, by the rule of a query's names; any other media type, or a body not given whole, gives its media type and no fields. The bytes are dropped once the names are read.
4. **A request the document left behind is closed when the next document of the tab loads,** by the loader it was sent under: one whose answer's head came is an exchange with no answer fields, one with none is forgotten. Without this a request whose page never read its answer would be awaited by every later take until its deadline, which the test showed.
5. **A screen is kept without its values.** When the address changes, by a new document or in the same one, and is on the tab's origins, the tab waits until the browser says the document is idle on the network (`networkIdle`) and no recorded request awaits its answer, or ten seconds (`SETTLE_BOUND`), and takes the snapshot `web_snapshot` takes, at most forty (`SCREENS_KEPT`). Every element's value is dropped, a password's mark kept; what a person typed is in a value and so never kept. An element's name stays as in any snapshot, redacted; the web probe turns a name that is data into its type (`screen::found`), so that rule stays in one place, the probe's.
6. **The first address is a checked type, `WatchStart`,** made only when it is on the origins (`surface_not_declared`) and does not look like an action (`address_looks_like_an_action`, the same name and class as the probe's). `watching_tab` takes it, so no caller can open the tab on an address that did not pass, and it adds no variant to `BrowserError`, whose every variant a crate above names.
7. **A watching tab records from the moment it opens.** It is opened for a capture alone, and the first page is part of it; `TrafficCapture::start` changes nothing.
8. **It may be waited on:** `changes`, `changed_since`, `looked_at`, `looked_at_since`, `is_open`, and `closed` are waits and reads, so that the capture job of package E2b can end when the person closes the window. None of them presses, types, navigates, or sends.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Intercept every request, as an observing tab does, and let each through | Every request of the person would wait on the engine, and one lost verdict would leave it paused; nothing is gained, since none is to be failed |
| Record every request, documents' `GET` and a page's images among them | A screen is already a snapshot; a page's loads are not the system's interface |
| Keep a screen's values, as a run's evidence does | A value is what the person typed or what the system holds of a record; decision 0392 keeps no value |
| Turn names into types in the base | The rule of which names are structure is the probe's, with its data lists; two copies would drift |
| Close a request left behind only at the take's deadline | Each later take would wait out its whole deadline once a page left one behind |
| A new `BrowserError` variant for an address that looks like an action | Every crate that names each variant would change for an error only the opening of this tab gives |

## Proof

`a_watching_tab_records_every_method_by_structure_and_the_engine_sends_nothing` (`crates/a2m-pack-web-browser/src/tests/watching.rs`), in a window on a screen (`A2M_REQUIRE_DESKTOP`, a virtual display on Linux), against the test application, the test acting as the person through the page: the page's `POST` search passes and its rows arrive; the summary page's `GET`, the line item's save by `PUT`, a body with a field named by a planted secret, and a form are each recorded with their bodies' names and kinds; the secret, the typed text, and the rows' text are in nothing the capture yields after the contract's check, and the field named by the secret is withheld and said to be; a move to an undeclared origin is ended; five snapshots are taken, one for each screen in order, none with a value; the stand heard only what the person did and the one address opened, and stored the save once. `a_watching_tab_has_no_method_that_acts` holds the type's public methods to their list.

## Revisit if

A browser stops putting a script's body in `Network.requestWillBeSent`, or sends `loadingFinished` for an answer its page never reads; a system is found whose reads a worker makes, which point 2 does not see; or the snapshot taken at `networkIdle` is found to miss what a page draws later.
