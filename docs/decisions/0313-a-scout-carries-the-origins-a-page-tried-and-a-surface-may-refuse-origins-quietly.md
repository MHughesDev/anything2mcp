# 0313. A scout carries the origins a page tried, a surface may refuse origins quietly, and the sign-in tab holds what a page loads

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-23

**Changes:** the discoverer `web_snapshot` and the `screen` finding of `a2m-pack-web-probe`; the drafting rules, the web surface's fields, the tab's setup, and the sign-in window of `a2m-pack-web-ui`; `Origins` and the sign-in tab of `a2m-pack-web-browser`; the test application's `/leaving/loads/orders/{order}`; the designs of the three crates, [section 301](../spec/3-packs/301-the-web-pack.md), [file formats](../reference/file-formats.md), and `SCOPE.md`. **Carries out** the list of "Not done (package P3b)" in [decision 0292](0292-a-web-tab-sends-only-to-its-surface-and-the-destinations-its-tool-declares.md), in part. No kernel crate, driver interface, provider manifest field, or file format of the kernel changes.

## Context

Decision 0292 made a tool's tab send only to its surface and the origins its operation declares, and left four things to a person and the browser: writing the list of origins, refusing an origin without stopping, the sign-in tab's subresources, and a name the browser looks up without a request. A person building over a page that loads a font from another origin met a stop, and had to find the origin and write it.

## Alternatives

**Where an origin refused without a stop is declared.**

| | Cost | Rules served |
| --- | --- | --- |
| **(a) A field of the web surface, `refuse_quietly`, a list of origins** | One list for every tool over the surface, written once. It only closes reach, so it needs none of the care a list that opens reach needs ([decision 0292](0292-a-web-tab-sends-only-to-its-surface-and-the-destinations-its-tool-declares.md) kept `permissions.destinations` for that) | Rule 2, rule 7; the pack's own surface fields are the pack's to define |
| (b) A field of an operation's `permissions` | One list for each tool, but `permissions` is the kernel's file kind: a new key changes the kernel's schema and the compiler | Rule 8 (knowledge of one kind of system lives in its pack) |
| (c) A field of a screen | Per page, which is where the origin is seen; but a screen is locators only | Rule 8 |

**Chosen: (a).** The refusal wins over a declaration: an origin that is both a destination and refused quietly is failed, so that one list cannot undo another. A value that is not exactly one origin with its scheme, or that is the surface's own origin, is a stop by name (`input/step_not_usable`, saying the surface "refuses quietly" it), never a guess.

**What the browser does.** A request to a quiet origin is failed before it leaves, as every request outside the tab's reach is, and is not reported; so the driver sees no refusal and the run goes on. A WebSocket or a WebTransport to such an origin is still held by the page's script and is still reported, and so still stops the run: only the requests interception holds are covered.

## Decision

1. **The scout carries the origins.** `web_snapshot` takes what its observing tab refused after each page; each refusal reported as `OUTSIDE` becomes an origin (scheme, host, port; no path, no query, no user information) in the `screen` finding's `refused_origins`, once, in order, and in the limitations as a `not_exercised` sentence naming them. The field is there only when there are some. Pages matched into one screen give the union. Everything else the tab refused is counted by method as before.
2. **The drafting rules propose them.** The rules that draft a read from a `screen` finding put `refused_origins` in the drafted operation's `permissions.destinations`, for a person to review, for both reads they draft from it. An entry that is not an origin written with its scheme and a host of plain characters (no path, no name or password, no wildcard), and the page's own origin, is left out. A screen carries none. An exchange finding does not carry origins yet.
3. **A surface may refuse origins quietly.** The web surface has an optional `refuse_quietly`, a list of origins with their scheme. The tab opens with them as a third set in `Origins` (`and_refused_quietly`); `reaches_reported` is false for them whatever else declares them, and a request to one is failed and not reported.
4. **The sign-in tab holds what a page loads.** The tab a person signs in in holds, besides its documents, the requests of the kinds the browser's interception filter accepts that carry no body: a style, an image, a media file, a font, a script, and the rest filed as `Other`. Each is held to the application's origin and the identity providers the target declares; one outside is failed, reported as `OUTSIDE`, and ends the sign-in with `policy`, `origin_not_declared`, saying to declare the origin under `identity_providers`. This closes the gap decision 0292 named: the person at the window is shown a named stop.

   Not held, and said so here and in the tab's design: a `fetch`, an `XHR`, and a beacon, which carry what the person types and which decision 0254 keeps out of the engine's process; the kinds this browser's filter does not name (measured: `TextTrack`, `Prefetch`, and `Manifest` are refused as unknown types); and a socket, a shared worker, and a service worker, because the sign-in tab has no script that holds them (measured: a socket of a sign-in page reaches the application). The publish page's reach is true without it, as decision 0292 said: a sign-in tab is not a tool's tab.
5. **A name looked up without a request is still not held.** Measured on this machine's headless browser: a page naming a loopback origin in `preconnect` and `dns-prefetch` hints, in its markup and from its script, produced no connection at a listener in three seconds. So nothing is built to hold it, nothing claims it is held, and the effect of a headed browser is not known. The roadmap row's last clause stays open.

## Proof

- `a_page_that_loads_from_the_other_name_gives_a_screen_that_names_that_origin_and_nothing_arrives` (`a2m-pack-web-probe`): the finding of `/escapes/loads` names the application's other name once, and the stand's count shows none of the page's loads arrived.
- `a_page_that_loads_from_its_other_name_drafts_a_read_that_declares_that_origin_and_it_runs` (`a2m-pack-web-ui`): from such a finding the drafted reads declare the origin; compiled, the plan's operation has it as a destination; without it the read stops with `origin_not_declared` and nothing arrives; with it the read is made and the image reaches the other name.
- `an_origin_the_surface_refuses_quietly_is_refused_and_the_read_goes_on` (`a2m-pack-web-ui`): refused quietly, with and without a declaration, the read is made and the stand counts nothing arriving; a refusal of another origin still stops; a value that is not an origin stops by name.
- `an_origin_refused_quietly_is_failed_in_the_browser_in_both_postures_and_reports_nothing` and `a_sign_in_tab_loads_nothing_from_an_origin_it_was_not_given_and_what_it_was_given_arrives` (`a2m-pack-web-browser`).

The whole path from a scout through the product's registry to a read, as `web_draft.rs` does it for an order's page, is not run for the page that loads from another name: it is in the product crate, outside this package. The two halves are each proven, and the join is the same finding as data.

## Consequences

- A person building a tool over a page that loads from other origins reviews a drafted list instead of writing one.
- A page's analytics or fonts can be refused for good by naming them once in the target, and the run goes on.
- A sign-in page that loads from an origin nobody declared now ends the sign-in by name, where before it worked. The origin is declared under `identity_providers`.
- Not done: the name looked up without a request; a refusal reported to the person while a quiet one is not (a count of quiet refusals is not kept); a lint at compile time for a `refuse_quietly` value, which is a stop at the run's start; the exchange findings carrying origins.
