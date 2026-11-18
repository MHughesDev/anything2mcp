# 0254. A person signs in in a window of the engine's own browser, and the web pack stops on every declared sign of expiry

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** the designs of [`a2m-pack-web-browser`](../architecture/crates/a2m-pack-web-browser.md) and [`a2m-pack-web-ui`](../architecture/crates/a2m-pack-web-ui.md), and the test application under `targets/example-app`. Carries out what [decision 0245](0245-a-web-application-is-signed-in-to-through-its-own-sign-in-page-with-a-credential-stored-for-the-person.md) left unbuilt: the sign of expiry in `page_http`, network events around a write and a sign-in (audit finding M7), way (b), and single sign-on as a person meets it in a browser. Builds on [decision 0248](0248-a-sign-in-goes-only-where-a-published-version-says-and-a-credential-belongs-to-its-target.md) and points 1 and 4 of [decision 0242](0242-the-owner-takes-the-recommended-answer-to-every-open-question.md). Where this record and decision 0245 differ, this record holds.

## Decision

### 1. `page_http` stops on a declared sign of expiry

- An `expiry_signals` entry of a web surface is `{screen: <name>}` or `{status: <code>}`, a client error from 400 to 499. Any other form, a redirect among them, is `step_not_usable` before anything is sent: the address a redirect answering a read from inside the page points to is not visible to the page (`fetch` with `redirect: "manual"` gives an opaque answer), so a redirect could only be guessed (rule 7).
- A `page_http` `goto` that lands on another address than the page it asked for, on a page that shows a declared screen's `ready`, stops with `session_expired`/`needs_login`, nothing sent. The look waits on the condition for at most five seconds, after the page has loaded.
- A read answered with a declared status stops with `session_expired`/`needs_login`, answered. An entry that reads the same status as a record or as its absence is refused at prepare, since the two could not be told apart.
- A write answered with a declared status stays in doubt (`possible_commit`), as every undeclared answer to a write does: the sign says the session ended, not that nothing was written. The read-back meets the expiry and stops by name.
- The `ui` driver heeds `{screen}` alone; it reads pages, not answers.
- **Proof:** `a_page_request_stops_by_name_at_each_declared_sign_of_expiry_and_reads_again_once_renewed` (`crates/a2m-pack-web-ui/tests/sign_in.rs`): the page answered with the sign-in screen, then a request refused with 401 by the stand's own guard after the page loaded (the stand's new fault `expire_sessions`); each stops by name, the keeper signs in once each time, the read succeeds; the same target without the status among its signs reads that 401 as `unexpected_status`; a redirect sign and a 302 are refused.

### 2. What crosses into the engine's process in an acting tab (audit finding M7)

Measured with a witness of every message the browser sends the engine, in this crate's tests only (`an_acting_tab_hears_no_body_of_a_write_and_its_cookies_and_token_only_where_named`).

- **A write's proof rests on no network event.** "Sent" and "answered" are the page's own request and its answer, returned by the helper through `Runtime`; `Dispatch` is the protocol writer's.
- **The network events cannot be switched off in an acting tab.** The browser passes the page's service worker by (audit finding M6) only while the tab's network events are on, and `Network.disable` undoes it: with them off, the service-worker test fails. Switching them off around a write would remove M6's proof, which this record does not do.
- **Narrowed to what M6 needs.** An acting tab enables its network events with `maxPostDataSize` of one byte: the browser puts no body of a request the page's script makes in any event (it reads 0 as no bound). The body of a write from inside the page, with a token sent in a body field, crossed before in `Network.requestWillBeSent` and now crosses in no event. The test failed before the change.
- **What still crosses, named.** The body of a form a document posts (a typed password) is in `Network.requestWillBeSent` whatever the bound, and in `Fetch.requestPaused`, the pause of a document request that holds the tab on its origins and cannot be left out. Cookies, a cookie-kept token, and a token in a header a write sets cross in `Network.requestWillBeSent`, `Network.requestWillBeSentExtraInfo`, `Network.responseReceivedExtraInfo`, and `Fetch.requestPaused`. None of these fields is parsed into any type or kept. The test fails if any other message carries them.
- **A sign-in window's tab hears no network event at all** (section 3): nothing in it writes, so it needs no service worker passed by.

### 3. Way (b): the person signs in themself, in a window

- `Visibility::Window { notice }` starts the session's browser afresh with a window, in the engine's own data directory and in-memory context, as every browser (rule 5). Its first tab is a page the engine writes, held to no origin, naming the target, the environment, and the identity; the browser's own first window, in its default context, is closed. A window is never used again, and a later open in the background closes it. On Linux the window needs `DISPLAY` or `WAYLAND_DISPLAY` in the engine's environment, passed to the browser with `XDG_RUNTIME_DIR` and `XAUTHORITY`; with neither the stop is `LaunchReason::NoScreen`, which the provider names `session_expired`/`needs_login`. On Windows and macOS the browser reaches the person's desktop with none of them.
- `Browser::sign_in_tab(env, look_on, pass_through)` holds the tab to both lists of origins, as an acting tab is held, and its helper looks at pages of `look_on` alone: on a page of `pass_through` every look is `ForeignOrigin` and reads nothing; its address is the browser's word (`Target.getTargetInfo`), and the page's title is not read. `ActTab::changes`, `changed_since`, and `wait_unless_changed` wait on a document loading or a refusal, a protocol event.
- The provider `window_login` takes `screen` (the sign-in screen, an address with no placeholder), `signed_in` (a screen whose `ready` is the sign of success), and `wait_ms` (the person's time, 5 minutes by default, 1 second to 30 minutes), and no credential. It tells the person (`PersonAsk::Notify`); with nobody there, no window opens. It goes to the sign-in screen and waits for the declared sign; it fills nothing and reads no field. A move to an undeclared origin is `policy`/`surface_not_declared`; a window or a download a page opens, `unexpected_page`; the time limit, `needs_login`. Then it exports the cookies of the application's origin, exactly as `form_login` does, and closes the window.
- `needs().person` is true: a renewal always waits for the person.
- **A draft may use it**, under an entry the person was shown on their page (`shown_to_person`, decision 0248): no stored credential is used.
- **A new provider name, not a setting of `form_login`.** `ProviderNeeds` is one value for a provider, and `form_login` needs no person; one name for each way keeps the keeper's view of "may this renew with nobody there" true.

### 4. Single sign-on through an identity provider's pages

- `sso_oidc` and `sso_saml` are way (b) with `identity_providers`, the provider's origins, written each as an origin with no path, separated by spaces. Each is returned by `destinations` (decision 0248), so the keeper sends nothing unless it is within the destinations the target declares, and checked again with `SignInContext::check_address` before the window opens. The tab may be on those and the application's origin; the engine reads nothing on the provider's pages, fills nothing anywhere, and exports only the application's origin's cookies. The two names are the same code: as a person meets them in a browser, both are pages and redirects.
- The test application has an imaginary identity provider, "Example ID", on a second port under the name `localhost` (the application is under `127.0.0.1`, so the two keep their cookies apart), a sign-in page, and a redirect back carrying a one-use code (`TestApp::start_with_identity_provider`).
- **Proof:** `single_sign_on_crosses_to_the_identity_provider_and_back_and_a_stray_origin_is_a_named_stop`: one sign-in at the provider, one code used, one session, a run reads under it, and the provider's own cookie is in no file and in no part of the session; a provider that sends the window to its own server under another name, an origin nobody declared, is stopped in the browser before the request leaves (the stand counts no arrival), with `policy`/`surface_not_declared`. `a_person_signs_in_in_a_window_from_drafts_and_the_engine_types_and_reads_nothing` does the same through the application's own page, from drafts. The person is played by a script of the stand's page (`person_at_the_window`), a route that is not the engine's: the engine sends no key and sets no value. Both ran here on a virtual display the test starts itself (`test_support::Screen`, `Xvfb -displayfd`); where none can be had the test prints "NOT RUN" and passes, and fails instead when `A2M_REQUIRE_DESKTOP=1`.

### 5. Not built

A session kept in page storage (item 5 of the package); the application's own sign-out; recognising a device-bound session (brief 30).

## After the second reading

The second reading of decision 0245 (audit 6, findings A1, A4 to A8) was checked against the code; each held.

- **A1, the session's cookies to a surface of another kind.** `form_login`, `window_login`, `sso_oidc`, and `sso_saml` refuse, in `check` and before any sign-in, an entry that covers a surface whose kind is not `web` (`input`/`auth_settings_invalid`, naming the surface). Test: `a_sign_in_in_a_browser_refuses_an_entry_that_also_covers_a_surface_of_another_kind`, which failed before. The other half, the HTTP and MCP packs refusing the header that carries the browser state, is another agent's.
- **A4, the password typed where the focus is.** Each value is now set on the element found and checked, by its node, through the element's own setter in the call that checks it (`Input::NativeSet`), never typed into what holds the focus. Before each value and before the press the page is proven again: the tab's document at the screen's declared path of the declared origin, by the browser's word; the screen's `ready`; and no frame at all, of the page or in the browser's frame tree, since a screen model declares none. Test: `a_password_goes_into_the_checked_field_alone_and_never_into_a_page_with_a_foreign_frame`: the stand's page moves the focus to a note field whenever the password field takes it; before the change the password landed in the note and the stand refused the sign-in; now the note receives nothing and the sign-in holds. A page with a frame from the identity provider's origin is a named stop with no attempt. The click's position-based press is unchanged: an element that moves under it is still the finding's open gap.
- **A5, a sign shown before the press.** The signs are looked for again just before the press, after the values are set. The look lasts 250 ms each time: a heuristic of timing, not a declared condition. Requiring evidence that the press was acted on (a new document) is not done: a sign-in page that answers without a new document would then always stop.
- **A6, the expiry sign shown by content.** Not done. Requiring the sign-in screen's path as well needs the observing tab's address, which `ObserveTab` does not give yet. The keeper's limit on renewals over time is `a2m-auth`'s: today a full sign-in from a stored credential is tried once per refusal, and nothing bounds renewals in a window.
- **A7, state not held to the run's origins.** `Browser::import_state` takes the run's `Origins` and refuses a state with a cookie for none of them (`ForeignOrigin`), placing nothing; the drivers take the state only from the web pack's own providers; `PlacedCookie` has no `Debug`. Test: `a_session_is_taken_out_placed_in_a_fresh_browser_and_never_written_to_disk` now places the state for another site's origins and is refused.
- **A8, the documents.** The crate documentation, the crate designs, and this record now say that `page_http` heeds the signs.
- **A test name that promised more.** `a_wrong_password_is_refused_after_one_attempt_and_never_tried_again` now asks for the sign-in twice more after the refusal; the stand still counts one attempt.

## What was weighed

- **Switching the acting tab's network events off for a write.** Rejected after measuring: it removes the service-worker bypass the write's arrival rests on.
- **A redirect as a sign of expiry in `page_http`.** Rejected: its target is not visible from inside the page.
- **The person played by a second debugging client or by synthetic input to the display.** Not available: the browser speaks only on its pipe, and no input tool is installed on the machines CI runs on. The page's own script is a route that is not the engine's.
- **Showing who the window is for in the window's title.** Not used: the browser's switch for a window name could not be confirmed in the program here; the notice tab works on every system.

## Consequences

- A person can sign in to a web application before anything is published, and through single sign-on, with the engine holding no password.
- `Browser::import_state` takes one more argument, the run's `Origins`. `Visibility` has a second variant. `LaunchReason` has `NoScreen`. `BrowserSettings` and `test_support` gain a screen for tests.
- Not run: anything on Windows and macOS; the window there relies on the CI runner having a desktop.
