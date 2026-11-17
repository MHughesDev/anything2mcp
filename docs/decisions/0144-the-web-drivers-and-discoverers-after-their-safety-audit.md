# 0144. The web drivers and discoverers after their safety audit

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the designs of [`a2m-pack-web-ui`](../architecture/crates/a2m-pack-web-ui.md) and [`a2m-pack-web-probe`](../architecture/crates/a2m-pack-web-probe.md), and two passages of [spec section 301](../spec/3-packs/301-the-web-pack.md): the rules every driver keeps, and the navigation row of "Discovery cannot write". Builds on decisions [0112](0112-the-ui-driver-as-built-five-step-kinds-and-a-commit-that-waits-for-its-confirmation.md), [0113](0113-a-screens-fingerprint-is-a-section-absent-is-a-variant-and-a-table-is-read-by-its-headings.md), [0114](0114-the-web-probe-as-built-three-discoverers-over-an-observing-tab.md), and [0143](0143-the-browser-engine-after-its-safety-audit.md), whose new tab interface the two crates now use.

## Context

A reviewer read the two crates against their documents and the ten rules, without running anything, and wrote two serious and five minor findings for the driver, and one serious and three minor for the probe. Each serious one was first shown by a test against the test application, through the driver handle or the discoverer, before it was fixed. The test application gained pages of the driver's own for it, under `/leaving/`.

## Decision

1. **U1, confirmed. The `ui` tab is opened for its surface's origin, and what the tab refused stops the run.** The browser engine ends a move to another origin and refuses to look at a page of one (decision 0143); the driver opens its tab for the surface's own origin only, and asks the tab what it refused at the start and the end of every step, and every half second while a click's answer is awaited. A move is `policy`, `surface_not_declared`; a window or a download is `unknown_variant`, `unexpected_page`; after a committing click either is `possible_commit`, `no_confirmation`. Proven by a page that moves itself to the other loopback name while it loads (the run stops before a field is set, and the other name receives nothing), and by pages that leave, or open a window, after the save was sent (the run is in doubt, and the application did store the save). *Instead of:* opening the tab for every destination of the run, which would let one surface's screens act on another surface's pages.
2. **U2, confirmed. A state the page showed before the click is never its answer.** A committing action is not clicked while the page shows one of the screen's variants, or already shows the action's confirmation: the failure is the variant's (`rejected` for `stop`, `unknown_variant` for `absent`) or `unknown_variant`, `unexpected_page`, and nothing was sent. After the click, a `stop` variant is the application's refusal; an `absent` variant, or a variant shown together with the confirmation, leaves the write in doubt. Proven by a page that shows a refusal once a field is typed, and one that says "Saved." before anything is done: no click, nothing stored. *Instead of:* clicking and counting only the variants that were not there before, which leaves a page that refuses again in the same words with no answer at all, after a write was sent. Whether the confirmation already holds is asked with a quarter of a second for the page to say so; a page that cannot answer in that time is taken as not showing it.
3. **P1, confirmed. Every address discovery visits passes the action check, whatever named it.** The check moved from the links into the one place that sends the tab, so a focus path, the surface's own address, and a link meet it alike. A redirect is followed inside the browser, so one that ends at such an address is refused after the visit, and is not read. A new code, `policy`, `address_looks_like_an_action`. Proven with the stand's route that writes on a visit. A redirect to another origin, which the browser now ends, is still seen as a sign-in by redirect.
4. **U3, confirmed, closed in the driver.** A locator with its own scope and a fingerprint is refused by the driver as by the compiler. That a fingerprint is a second look at the page is stated as a limit.
5. **U4, U5, U7, P2, P4: limits stated.** The read-back is a new visit in the same profile, which an application that keeps edits in the page's own storage could answer from its copy. Runs with no session share one profile per target and environment, as discoveries do; a lease whose profile has the name of either is refused. Page text in captures and titles in findings are evidence, not marked untrusted here. A page that moves after it was reached is now held by the browser, and what it tried is a limitation on its finding.
6. **U6, confirmed, closed by 1.** An undeclared window or download after an `invoke` stops the run.
7. **P3, confirmed.** A `page_unreadable` message names the kind of answer and holds none of its text; a page title in a finding is cut to 160 characters.
8. **The other fixes of this audit, for these two crates.** The posture guard lets a run against a test record set fields and perform only the action marked as the commit point (decision 0147). The `ui` lowering states that only an `invoke` may save, as its action declares, and a binding's least effect by the HTTP pack's rule, so that one verb means one effect in every mode: `submit` is `irreversible`, and so is a write with no `read_back`. A screen with any action describes a write. The manifest lists no `commit_steps`, and both packs declare `browser_digests` and read it. `page_http` is not built; its lowering is to take the HTTP pack's rules when it is (decisions 0139 and 0154).

## Consequences

- A screen whose page shows a success message on arrival needs a confirmation that tells a new save from an old one.
- An operation bound with `submit` through the `ui` mode declares `effect: irreversible`.
- A discovery focus that names a path holding an action word stops the discovery instead of being looked at.

## Not closed

- A refusal the page shows a moment after the click for a reason of its own is taken as the application's answer.
- A redirect to an address that looks like an action is visited before it can be refused.
- Two discoverers that both register read the browser digests from whichever registers first.

## Revisit if

- The browser engine offers a read-back with its cache and the page's storage set aside, or a hook on each hop of a redirect.
