# 0415. A request whose answer is being read is closed by the read alone

**Status:** Accepted. Design decision, made by the worker who found the fault, under the lead's brief. The owner had not read this record when it was merged. It amends point 4 of [decision 0411](0411-the-watching-tab-records-from-the-browsers-network-events-and-keeps-each-screen-without-its-values.md) and changes no public interface.

**Date:** 2026-11-24

**Changes:** `Page::answer_to_read`, `Page::left_behind`, and the `Pending` record of a request in `a2m-pack-web-browser` (`tab.rs`, `tab/watching.rs`).

## Context

A watching tab closes, when the next document of its tab loads, what the documents before it left behind (decision 0411, point 4). A request whose answer has arrived whole (`Network.loadingFinished`) is read off the tab's stream of events: its body is fetched by a call (`Network.getResponseBody`) on a task of its own, and the request stays among those awaited until the read ends. Nothing told the close that the read was on its way. When the next document loaded while the read was still out, the close made the request an exchange with no fields, and the read, ending after it, made it an exchange again: one request the person's use made was recorded twice.

Measured in the row's test, `a_watching_tab_records_every_method_by_structure_and_the_engine_sends_nothing`, under load on Linux (the crate's other tests looping and two cores busy, as CI's window job runs it on a virtual display): 6 failures in 60 runs before the change, each with more `POST` searches than the page made (five or six where it made four), never fewer. The extra ones were always among the two requests the test sends just before the person moves on, the body with a field named by the planted secret and the form, each answered `400`: one recorded twice in five runs, both in one, a doubled copy often after a request that was sent after it, which shows its read was still out when the next document loaded.

## Decision

1. **A request whose answer has arrived whole is marked as being read, on the tab's one stream of events, before its read starts.** The mark is set under the same lock the close takes, and the stream hears the next document only after it, so the close cannot miss it.
2. **The close leaves a request being read to the read,** which records it once, with the names and kinds of its answer when the body was read, and with none when the read failed; it is awaited until then, within the tab's time limit for a call. Everything else point 4 of decision 0411 says stands: an answered request with no read under way becomes an exchange with no fields, and one with no answer is forgotten.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Let the read record the request only if the close has not | The answer came whole and its read is under way; recording it without fields throws away what the read is about to give |
| Read the body on the stream of events itself, so that no document is heard before the read ends | Every event of the tab would wait on a call to the browser, which is why the read was moved off the stream |
| Make the test wait longer before the person moves on | The fault is the capture's: a person moves on when they like, and a capture that doubles a request drafts from a false picture |

## Proof

The same test, in the same loop under the same load, passes 100 runs of 100 after the change. Its assertion that the capture holds exactly the four searches the page made is unchanged.

## Revisit if

A request with no answer yet when the next document loads is found to have been answered after all: decision 0411 forgets it, and a capture would then miss a request the person's use made. No run of the test has shown it.
