# 0399. The web pack handles the edge, a fixed language, and the pages of cases 22 to 27

**Status:** Accepted. Design decision. The owner had not read this record when it was written. Carries out package 8.5 for the cases of the test application that the web pack's own crates handle, and writes down what is left with another owner.

**Date:** 2026-11-24

**Changes:** the `ui` driver's `goto` and health; the browser base's `Loaded` and fixed environment; the edge-case table of [spec section 301](../spec/3-packs/301-the-web-pack.md); the cases of [the test application's plan](../plan/test-application.md); four failure codes (`challenge_page`, `rate_limited_by_edge`, `forbidden_by_edge` in the web pack, and `surface_in_maintenance`).

## Context

Package 8.5 is "the edge-case handlers of spec section 301", proved by cases 22 to 27 of the test application. Cases 6 to 12 and 33 were proven before, by packages 1.6 and 8.4. For each of the six, the question was which part is the web pack's and which belongs to the engine or to another pack.

## Decision

1. **The edge's own answer to a page is a stop, never a retry, and never an answer to a challenge.** When a `goto` is answered with status 403 the code is `forbidden_by_edge`; with 429, `rate_limited_by_edge`; with a document response that carries the header `cf-mitigated: challenge`, `challenge_page`; each class `edge_blocked`. A 503 is `surface_in_maintenance`, class `busy`, which passes by itself. The browser base reports the status and whether the document was held for a challenge (`Loaded::challenged`); it reads that one header and keeps no other. The failure says the edge answered, with the status, and the step's fate is `answered`. The engine does nothing to get past a challenge (rule 5): it does not solve one, wait it out in the page, or change what it looks like.
2. **A surface that was refused is left alone for sixty seconds.** The `ui` driver remembers, per origin, the refusal that stopped a visit. Any `goto` to that origin in that time fails with the same code before the tab is asked, with nothing sent, and the driver's `health` for the surface is `Unavailable` with that code. This is the pack's half of "no retry storm". The engine's use of `health` to refuse or delay a run is the kernel's, and is not changed here.
3. **The fixed locale is also what the page is told it reads.** The base sets `Accept-Language` and `navigator.language` to the target's locale, beside the number and date formats it already fixed, so a page that chooses its language by what the browser says chooses the target's. An operation compares a choice by its code (the `select` rule's value, and the `options` of the controls of [the widget-driver record](0399-the-remaining-widget-drivers-built-from-what-a-page-says-of-its-controls-a-staged-file-a-grid-cell-by-its-rows-key.md)), so the language of a label does not matter, and a target in another language works with the same operations.
4. **A page that signs its own request is driven as the page.** The `ui` driver presses the page's own button, so the request leaves the page with its own signature (case 22). What the test application proves is that the signed request is stored and that the same request from outside the page is refused. That the HTTP binding of such an endpoint is marked unsupported, and that the scout's endpoint mining stops at the page's script, are the HTTP pack's and the probe's, and are left with them.
5. **A job that finishes later is waited for as any confirmation is.** An action's `confirm` is a condition the page comes to show, within the step's time (case 24). A link the page offers as a download is a download no screen declares: the browser writes no file for it, and the run stops, as before. A download a step declares, through a folder inside the engine-owned profile, is not built: it needs the browser to be told, for one step, that one download may be written, which changes a browser-wide refusal the engine has always held, and that is its own decision. It is left with `a2m-pack-web-browser`, with the staging folder of [the widget-driver record](0399-the-remaining-widget-drivers-built-from-what-a-page-says-of-its-controls-a-staged-file-a-grid-cell-by-its-rows-key.md) as its likely place.
6. **A layout nobody declared is a stop.** A field's locators are tried in order, and the first that matches is used (case 25). A page whose layout shows none of the screen's `ready` conditions is `unexpected_page`, with nothing typed.
7. **Text with instructions in it is a value.** A note read through the `ui` driver is returned as text, of an output of the type `untrusted_text`, and no step presses or types anything because of what it says (case 27). The wider manipulation test suite of spec section 502 is the kernel's.

## Left with another owner

| What | Owner |
| --- | --- |
| Marking the HTTP binding of a request the page signs as unsupported; the scout's mining stopping at the page's script (case 22) | The HTTP pack and the probe |
| A download a step declares, into the profile (case 24) | `a2m-pack-web-browser`, by its own decision |
| The engine's refusal or delay of a run on a surface whose driver is `Unavailable` (case 23) | The engine |
| A run-wide limit on how often a surface is asked | The kernel ([spec section 225](../spec/2-kernel/225-performance.md)) |

## Alternatives

| Alternative | Why not |
| --- | --- |
| Treat a 403 as a variant a screen declares | A screen would then say what the edge does; the edge is not part of an application's screens, and a block must be the same stop on every screen |
| Retry a 429 after its `Retry-After` | A retry is what a rate limit asks a client not to do too soon; a person is told, and the hold stops a run from asking again within the minute (rule 5, and the section on bot defences) |
| Set only `Intl`'s locale for the fixed language | A page that reads `navigator.language` or the header would still say the machine's |
| Allow every download in the profile | The refusal of downloads is a safe default; a download is allowed for a declared step only |

## Proof

In `crates/a2m-pack-web-ui/tests/later.rs`, through the driver and the screens of `tests/fixtures/cases`, against the pages of the test application, each asking the application what it was told: the signed memo stored and the unsigned one refused; each of the four edge pages a named stop, answered, with the surface held off for any page of it and the stand asked once; the export waited for, and the link refused as a download; both layouts used and a third a stop; the unit set and read by code in `en-US` and `de-DE`, with the language and the number the page showed; the note read as untrusted text with nothing pressed.

## What would reopen it

An edge that answers a refusal with the status 200, which the pack could tell only by its page; or a surface whose block ends in less than a minute and whose owner wants a run to try again sooner.
