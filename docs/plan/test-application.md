# The test application and the driver conformance suite

Anything2MCP is never tested against a real business application. Everything is proven against one purpose-built application that imitates the hard cases, and against a fixed list of behaviors every interface driver must pass. Package 0.3 built the first cases and later packages added more; [What is built](#what-is-built) lists each.

## Principles

- **Ground truth comes from the application, not from Anything2MCP.** The test application has a control interface that reports what it has actually stored. A test that asks "was the record written once?" asks the application, never Anything2MCP's own read path.
- **Every fault can be switched on.** Slow responses, dropped connections, conflicts, and blocks are settings of the test application, so a failure is reproduced on demand.
- **It names no real product.** It is an imaginary order-entry application, the same `example` target used throughout the documents.
- **It is ordinary and small.** A Rust web server with plain pages and no build step for the first cases. Pages written with particular front-end frameworks are added in Phase 8, where each framework's behavior is the thing under test.

## The control interface

Available only in the test application, under its own path.

| Call | Purpose |
| --- | --- |
| Reset | Restore known data |
| Inspect | Return what is stored for a record, exactly |
| Count writes | How many times each record was written, and by which request |
| Set a fault | Turn on one of the faults below, for the next request or until cleared |
| Set a mode | Session timeouts, single-session mode, a variant flag, the locale |

## Cases

Each case exists because of a named risk. *Phase* is when Anything2MCP must pass it.

| # | The application does this | Risk it exercises | Phase |
| --- | --- | --- | --- |
| 1 | Serves a list, a detail page, and a form over a JSON interface | The basic read and write path | 0 |
| 2 | Rejects bad input with a validation message | `rejected` is returned with the message | 0 |
| 3 | Uses a version on each record and refuses a stale write | Version preconditions; `version_conflict` | 0 |
| 4 | Has an endpoint that answers slowly, and one that fails some of the time | Waits; `transient` and retries | 0 |
| 5 | Posts forms with an anti-forgery token that changes per request | Token fetching in the HTTP modes | 4 |
| 6 | Re-renders part of the page on every change, with identifiers that differ on each load | Locators that do not rely on generated identifiers | 1 |
| 7 | Ignores a value set on an input unless real input events fire | The widget driver's escalation from fill to typing | 1 |
| 8 | Changes the page address without loading a new page | Readiness after in-page navigation | 1 |
| 9 | Shows a grid of ten thousand rows of which only the visible ones exist in the page | Finding a row by key, not by position | 1 |
| 10 | Uses custom controls: a filtering dropdown that loads options late, a date picker, a rich-text editor, a file input, a toggle, a tree. Built as the page `/cases/controls`: two filtering dropdowns, a date and time field, a time field, a date field that can only be picked, a slider, a file input, a rich-text editor, a switch, tabs, an accordion, a tree, and a grid edited in place; "Save options" tells the application what the page holds, as one `saved` event of JSON ([decision 0351](../decisions/0351-the-remaining-widget-drivers-built-from-what-a-page-says-of-its-controls-a-staged-file-a-grid-cell-by-its-rows-key.md)) | One widget driver each | 1, then 8 |
| 11 | Shows a confirmation dialog, sometimes | Declared variants; `unknown_variant` when undeclared | 1 |
| 12 | Puts a form inside a frame, and a control inside a shadow tree | Frame paths and shadow scopes | 1 |
| 13 | Signs in by form, by redirect to a separate identity page, and with a one-time code step | Auth providers and discovery | 2 |
| 14 | Ends a session after a short idle time, and after a fixed total time | The session keeper's probes, renewal, and `needs_login` | 2 |
| 15 | Allows one session per account when told to | Detecting exclusive sessions | 2 |
| 16 | Stores a value differently from what was sent: rounds it, reformats it, recalculates another field | Normalization and `mismatch` | 3 |
| 17 | Commits a write and then drops the connection before answering | `possible_commit` and reconcile | 3 |
| 18 | Assigns the identifier on create, with and without a spare reference field | Identity strategies: `stamped` and `find` | 3 |
| 19 | Has a header with lines, saved separately, and another saved in one request | Dependent records; one commit point with nested records | 3 |
| 20 | Creates a duplicate if the same create is sent twice | That Anything2MCP never sends it twice | 3 |
| 21 | Lets a second user change a record between preview and write | `changed_since_preview` | 3 |
| 22 | Computes a value in the browser before saving, and signs a request in the browser. Built as `/cases/signed` ([decision 0355](../decisions/0355-the-web-pack-handles-the-edge-a-fixed-language-and-the-pages-of-cases-22-to-27.md)) | Endpoint mining stopping at shadow; the HTTP binding marked unsupported | 4 |
| 23 | Answers with a rate-limit status, a challenge page, or a maintenance page when told to | `edge_blocked` and surface health; no retry | 4 |
| 24 | Starts an export that finishes later and offers a download. Built as `/cases/export`, which offers a small file and one of two mebibytes whose suggested name climbs out of a folder | Long-running jobs and downloads | 4 |
| 25 | Shows one of two layouts depending on a flag. Built as `/cases/layout/{flag}`, with a third layout for a flag nobody declared | Ordered locators; a new variant stops | 8 |
| 26 | Switches language and number format. Built as `/cases/locale`, which speaks the browser's language | Fixed locale; comparison by code, not label | 8 |
| 27 | Returns text containing instructions aimed at an agent. Built as `/cases/note`, with a button the text names | Untrusted fields; the manipulation test suite | 3 |
| 28 | Exists as a small native desktop program with the same data. Built as `targets/example-desk` for Linux: a test hosts it, and it publishes an accessibility tree over a session bus of the test's own and draws no window. It holds the labelled values of one order, a table of its line items, a control with no accessible name, two controls of one role and name, a password field, a value that appears late, and a Save button, and counts each press of Save as a write and each ask for the password's text ([decision 0198](../decisions/0198-the-desktop-pack-reads-linux-applications-through-a-look-only-back-end.md)). On Windows the same crate builds a window of plain controls in a process of its own, shown on screen without taking the focus, with the same data, and counts each message a client sends its controls ([decision 0209](../decisions/0209-the-desktop-pack-reads-windows-applications-through-ui-automation-on-a-thread-of-its-own.md)); on macOS a window of AppKit's controls, through the same harness ([decision 0244](../decisions/0244-the-desktop-pack-reads-macos-applications-and-proves-the-record-before-every-act.md)). Each learned to be written: edit fields apart from the values that show what is stored, a Save that stores by one rule, a record of every value set, and faults a test switches on, on Linux ([decision 0218](../decisions/0218-the-desktop-pack-writes-on-linux-through-a-second-closed-list-and-reads-back-what-is-stored.md)) and on Windows and macOS, where it has not yet run ([decision 0399](../decisions/0399-the-desktop-pack-writes-on-windows-and-macos-through-a-closed-list-of-its-own-on-each.md)). On Linux it also serves itself as a program of its own when a copy of a test binary is started under one of its two names, publishing its application or nothing, and writes its process number into a marker a test reads; and its `New status` field saves on Enter, its one action `activate` bound to Return ([decision 0467](../decisions/0467-the-desktop-pack-starts-an-application-a-surface-allows-does-an-action-by-a-key-the-element-binds-and-probes-its-reach.md)). On Windows it serves itself the same way, from a start function in the C runtime's initializer section, showing its window with no control interface or showing nothing ([decision 0480](../decisions/0480-the-desktop-pack-starts-an-application-on-windows-by-the-event-of-a-window-shown-and-not-yet-on-macos.md)) | The desktop driver | 8 |
| 29 | Takes a bearer token or an API key on its JSON interface, and revokes one when told to | Credentials in the HTTP modes; a refused credential is not tried again | 2 |
| 30 | Serves a description of its JSON interface | Reading a described interface; the description held to what is served | 4 |
| 31 | Answers in a changed shape when told to | An unknown response shape stops, never guessed at | 1 |
| 32 | Has pages that write by themselves: on load, from a worker, over a WebSocket, and behind links that look like actions | That discovery never writes | 5 |
| 33 | Uses the browser's own controls: a select, a check box, a radio group, a date field, a text area, a confirmation dialog before saving, and a button disabled until a field is valid | One widget driver each for the native controls | 1 |
| 34 | Redirects to another host name | A redirect off the target stops; no input changes where a request goes | 2 |
| 35 | Answers an optional object property as an object, as `null`, as missing, or as a text, when told to; its description lists the property as not required and not nullable | A property the description does not require is drafted as possibly null; a value of another kind still stops ([decision 0297](../decisions/0297-a-property-a-description-does-not-require-is-drafted-as-possibly-null.md)) | 2 |
| 36 | Answers with a property named `client_secret`, with an invented value, beside ordinary values, and takes an optional query value `access_token`; a second read requires a query value `api_key` | A field or a parameter named as a secret is never drafted as an output or an input; the read that requires such a parameter is not drafted ([decision 0302](../decisions/0302-a-field-named-as-a-secret-is-never-drafted-as-an-input-or-an-output.md)) | 2 |
| 38 | Serves its description revised, as a release would revise it, when told to | A read pinned to the description's first text stops as `drift`, `description_changed`, before its call is sent ([decision 0389](../decisions/0389-the-api-mode-binds-a-read-to-a-pinned-description-and-names-its-change-as-drift.md)) | 2 |
| 39 | Has a page whose script asks for its rows with `POST` and a JSON body, at an address that reads or archives a row by one field of that body. Built as `/cases/rows-by-post` and `POST /api/cases/rows/search` ([decision 0394](../decisions/0394-an-asserted-read-of-a-web-surface-is-a-page-request-matched-by-one-shared-shape-and-shown-at-publish.md)) | An observing tab lets through a request a person asserted safe only when its method, origin, path template, and body all match; an `archive` body, an unnamed field, a field of another kind, a value outside the `enum`, and a request no one listed are failed before they leave | 3 |

## The driver conformance suite

The interface driver is the riskiest piece of engineering, and the design keeps a fallback: a different engine behind the same driver interface. The fallback is only real if both can be judged by the same measure. The conformance suite is that measure: a numbered list of behaviors, each a test against the cases above, that any implementation of the `ui` driver must pass.

| Group | Behaviors |
| --- | --- |
| Launching | Start the installed browser with an engine-owned data directory; start a pinned browser; survive a browser crash |
| Locating | Each locator kind finds exactly one element; an ambiguous match fails; a fingerprint mismatch fails as `drift` |
| Waiting | An element that appears late is noticed without polling; every condition in the file format reference; a timeout raises the declared class |
| Acting | Each widget driver sets and reads back its control, including cases 7, 9, and 10 |
| Frames and shadow trees | Case 12 |
| Network | Observe the response for a named endpoint; make a request from inside the page with the page's own credentials |
| Snapshots | The same page gives the same snapshot twice; restricted values are masked |
| Isolation | Two identities never share a process or a profile; two runs on one identity share a sign-in and nothing else they should not |
| Speed | Each budgeted path in spec section 225, measured |

Results are kept as a table per implementation and operating system. The Phase 0 spike fills in the first rows for each candidate base; choosing a base means comparing those tables.

## What is built

As of slice 0.3, on 2026-11-16: cases 1 to 4, in `targets/example-app/`. Its [README](../../targets/example-app/README.md) lists every address, the rules a write must pass, the control interface, and the seed data.

- **The control interface** has Reset, Inspect, Count writes, and Set a fault. Set a mode is not built; the first case that needs it is case 13.
- **Faults** so far are a delay and a status. Dropping a connection after a commit arrives with case 17.

As of 2026-11-17 these were added, each without changing anything that was there. The README lists the addresses and the control calls.

| Case | What is built | What is not |
| --- | --- | --- |
| 5 | Proven for the HTTP pack by `crates/a2m-pack-http/tests/forgery.rs` and `crates/anything2mcp/tests/forgery_write.rs`: the page is fetched, its token sent once in a header or a body field, and an answer that quotes it is not quoted back ([decision 0357](../decisions/0357-the-http-pack-fetches-the-page-that-gives-a-writes-token-and-names-the-edge-when-it-answers.md)). The note of an order on `/orders/{order}/note`, whose page holds a fresh anti-forgery token in a meta element, a hidden input, and a cookie its script may read; `PUT /api/orders/{order}/note` takes it once, in the header `X-CSRF-Token` or the body field `authenticity_token`, and refuses a write without it, or with one used or not given, with 403; a refused note is answered with 422, quoting the body it was sent. `/__control/notes` reports notes, writes, refusals, and tokens ([decision 0206](../decisions/0206-the-page-http-mode-writes-by-put-with-the-pages-own-token-in-an-acting-tab.md)). The same page and interface are served behind the stand's session at `/secure/orders/{order}/note` and `/secure/api/orders/{order}/note`, over the same notes ([decision 0373](../decisions/0373-the-page-http-write-stops-at-the-edge-and-at-an-ended-session-before-it-sends.md)) | Forms posted as documents; a token bound to a session |
| 10, 11 | Case 33 covers the browser's own controls; its page always asks for confirmation | A dialog that appears only sometimes |
| 13 | Sign-in by form under `/secure`, a session cookie, sign-out, a lockout after five failures in a row, and a one-time code step switched on by a mode. The control interface counts attempts, failures, and refusals | Sign-in by redirect to a separate identity page |
| 14 | The control interface expires or revokes every session | Idle and total time limits on a clock |
| 15 | A mode in which a new sign-in revokes the other sessions | |
| 17 | The fault "lost after commit": the write is stored and the connection closes with no answer. It covers the JSON interface and so the pages' saves, which go through it | |
| 23 | Fixed routes for a 403 edge page and a 429 with a retry time; the status fault gives either on any path; `/edge/challenge` (403 and the header `cf-mitigated: challenge`), `/edge/maintenance` (503 with a retry time), and `/edge/ok`. Each asked is listed by `GET /__control/cases` as `edge: <name>`. Proven for the HTTP pack by `crates/a2m-pack-http/tests/edges.rs` ([decision 0357](../decisions/0357-the-http-pack-fetches-the-page-that-gives-a-writes-token-and-names-the-edge-when-it-answers.md)) | A challenge page and a maintenance page | |
| 29 | The whole JSON interface again for a bearer token and for an API key header, with 401 and `WWW-Authenticate`; the control interface revokes either | |
| 30 | An OpenAPI 3.1 document, written by hand; a test calls every operation it names and compares its list with the served routes | The signed-in copies are named in its text, not as paths |
| 31 | The fault "changed shape" renames one field in a JSON answer | |
| 32 | The four trap pages, each write counted by name | The WebSocket route refuses the upgrade after counting the handshake, so no socket opens |
| 32 | The `/leaving/{how}/orders/{order}/items/{item}` pages for the interface driver ([decision 0144](../decisions/0144-the-web-drivers-and-discoverers-after-their-safety-audit.md)): the line-item form that moves itself to the other loopback name on load (`load`), after Save (`click`), or opens a window on it (`window`), shows a refusal once a field is typed (`stale`), or says "Saved." before anything is done (`saved`), or loads an image and a `fetch` from the other name (`loads`). Every move aims at a trap route that counts what arrives. And `/escapes/loads`, a page that loads an image, a script, a style, two `fetch` requests, and a socket from an origin it is given, optionally from inside a frame of another origin, for the proof that a tab sends nothing outside its reach ([decision 0292](../decisions/0292-a-web-tab-sends-only-to-its-surface-and-the-destinations-its-tool-declares.md)) | |
| 33 | The delivery-details page and its JSON routes | Proven through the interface driver: each native control set and read back, the browser's own confirmation answered where the action declares it, and the refusals that store nothing ([decision 0350](../decisions/0350-the-phase-1-cases-of-the-interface-driver-frames-answered-questions-native-dialogs-two-more-controls-and-what-the-grid-still-needs.md)) |
| 6 to 9, 11, 12 | The pages under `/cases`, and `GET /__control/cases`, which lists what each page was told, in order: `/cases/rerender` (the list drawn again with new identifiers), `/cases/typed-input` (keeps only what key presses made), `/cases/in-page` (the address moves with no new document), `/cases/grid` (ten thousand rows, about fifteen in the page, and a box that brings a row in by its key), `/cases/confirm` (a question on some presses, then the browser's own), `/cases/notice` (a notice over a form), `/cases/frame` with `/cases/frame/inner` (a form in a frame, and a control in an open shadow tree), and `/cases/controls` (case 10, the custom controls) | A frame of another origin |
| 34 | A redirect to the other loopback name on the same port | |
| 8.4a | `/cases/commit/plain` (a change taken when the control loses the focus), `/cases/commit/postback` (a change posted to the application, recorded as `posted`, and the form drawn again from its answer), and `/cases/commit/stuck` (the same form, never drawn again), each with a button that saves; with `/cases/typed-input`, the pages of the input commits of platform profiles ([decision 0385](../decisions/0385-the-set-step-commits-a-text-control-as-the-surfaces-platform-profile-says.md)) | A whole page posted back |
| 8.2a | `/cases/site-tools`: a page that registers tools for agents with `document.modelContext.registerTool`: one that saves a note and tells the application, one that gives a total and says it only reads, one that fails, and one registered a moment after the page loads ([decision 0359](../decisions/0359-the-site-tools-mode-calls-a-tool-a-page-registers-by-its-name-and-digest-as-a-write-that-cannot-be-taken-back.md)) | |
| 9.1 | `POST /__control/screens` changes the form of a line item as a release would, until a reset: a label and a button reworded, the classes and identifiers renamed with a style sheet added, the fields reordered, or the form wrapped in a named card that takes its heading; or two labels reworded at once (`relabel`, [decision 0999](../decisions/0999-a-drift-yields-a-repair-bundle-and-a-person-accepts-a-repair-a-pack-proposed-by-rule.md)), which no rule of exact pairing can repair. Proven through the product by `crates/anything2mcp/tests/screen_drift.rs` ([decision 0384](../decisions/0384-a-screen-changed-on-purpose-is-caught-by-its-locators-or-its-fingerprint-and-a-named-section-is-a-region.md)) | A framework upgrade; a test id on the form |
| 35 | `GET /api/orders/{order}/shipment`, described by the schema `Shipment`; `POST /__control/shipment` with `{"answer": "object" \| "null" \| "missing" \| "text"}` sets how its `carrier` is answered until the next reset | |
| 36 | `GET /api/orders/{order}/handoff`, described by the schema `Handoff`, whose `client_secret` is the invented text `example-not-a-secret-0001`, with an optional query value `access_token`; `GET /api/orders/{order}/pickup`, which requires a query value `api_key` | |
| 38 | `POST /__control/description` with `{"revised": true}` serves `/api/openapi.json`, and its copy under `/secure/bearer`, with `info.version` `2`, until a reset or `{"revised": false}`; `TestApp::revise_description` | Case 37 is the hand-off's optional `packages`, answered by the switch of case 35 |
| 39 | `/cases/rows-by-post`, whose script sends `POST /api/cases/rows/search` with `{"status": "open", "op": "search"}` and shows the rows, or `Failed.`; the route answers the rows of a status for `op: "search"` and archives the first of them for `op: "archive"`; `GET /__control/case-rows` (`TestApp::case_rows`) reports the rows as stored and the `op` of every body the route was sent. Proven for the browser base by `an_observing_tab_lets_through_a_safe_request_only_on_method_origin_template_and_body` ([decision 0394](../decisions/0394-an-asserted-read-of-a-web-surface-is-a-page-request-matched-by-one-shared-shape-and-shown-at-publish.md), package E7b) | A form body; a search whose field is a whole question |

Set a mode now exists for signing in only: the one-time code step and one session per account.

The fault "redirect after commit" was added after case 17 ([decision 0139](../decisions/0139-a-write-that-was-sent-is-never-settled-by-an-answer-that-is-not-the-applications-verdict.md)): the write is carried out and stored, then the answer is `303` with a `Location` the test names. It applies to requests that may write, as "lost after commit" does, and it shows that a write answered by a redirect is not settled as unsent.
- **The harness** is `example_app::harness::TestApp`. A test starts an application on a free port, uses it over HTTP, and asks the control interface what was stored and how many times. Two applications in one test share nothing.
- **Lengths** are stored as whole sixteenths of an inch and travel as decimal text, so the application itself never holds a float.
- **A line item that does not exist** has no form at its own address; the form for a new one is at a different address. That is the shape hypothesis H-23 is about.
- The line-item form is driven by the interface driver's tests, in `crates/a2m-pack-web-ui/tests/driver.rs`, in a browser the engine starts: a write stored once and read back, and a write the application refuses.
- The product's crates may use the application in their tests only. `tools/docs_audit.py` refuses it, and every other crate under `targets/`, as an ordinary dependency.

As of 2026-11-17 the stands for the other kinds of system are built too. Each follows the principles above, and its README lists its cases, its control interface, and its faults.

- **The command-line stand**, `targets/example-cli/`: its [README](../../targets/example-cli/README.md).
- **The database stand**, `targets/example-db/`: its [README](../../targets/example-db/README.md).
- **The file stand**, `targets/example-files/`: its [README](../../targets/example-files/README.md).
- **The broker**, `targets/example-broker/`: its [README](../../targets/example-broker/README.md).
- **The simulated device**, `targets/example-device/`: its [README](../../targets/example-device/README.md).
- **The MCP stand**, `targets/example-mcp/`: its [README](../../targets/example-mcp/README.md).
- **The decompiling-engine stand**, `targets/example-decompiler/`: a program that is handed a file and prints one JSON line for each function it made text of, in the modes `fail`, `garbage`, `no_end`, `secret`, and `flood`; it records every start ([decision 0343](../decisions/0343-the-decompiling-engine-is-a-discoverer-of-the-provider-contract-that-starts-only-a-pinned-program-and-reads-one-form.md)).
- **The NATS stand**, `targets/example-nats/`: its [README](../../targets/example-nats/README.md). Its faults include `ExpireAuthAfterStore`, `ExpireAuthAfterAck`, and `ForeignDelivery` ([decision 0184](../decisions/0184-the-data-and-stream-packs-after-their-second-safety-audit.md)), and `ConsumerWithoutAckWait`, which describes a consumer without its `ack_wait` ([decision 0197](../decisions/0197-no-client-takes-a-proxy-from-the-environment-and-stand-only-protocols.md)). The database stand's relay has the fault `HoldCommitAnswer`, which holds the answer to `COMMIT` until the step's deadline.

The application stand has pages whose own script reads from the JSON interface as it loads, for the web pack's drafting of a page's own requests ([decision 0195](../decisions/0195-a-read-never-runs-where-the-page-may-write-and-a-page-keeps-only-its-structure.md)):

- `/orders/{order_id}/summary` asks for the totals with no query and no header of the page's own.
- `/orders/{order_id}/lengths-by-query` and `/orders/{order_id}/lengths-by-header` ask for the total length in centimetres, by a query and by a header the script sets. `/api/orders/{order_id}/lengths` answers in inches unless one of them asks, with the same names either way.
- `/notes/{note_id}` is one screen at two addresses; the page of `N-2` alone sends a write as it loads, which the control interface counts.
- `/invitations/{token}` holds a long random text in its address, and its script sends that text in a request's path and as a bare entry of a query.
- `/cases/rows-by-post` (case 39) asks for its rows with `POST` and a JSON body, from an address that reads or writes by the body's `op` ([decision 0394](../decisions/0394-an-asserted-read-of-a-web-surface-is-a-page-request-matched-by-one-shared-shape-and-shown-at-publish.md)).
- `/orders/listed` shows the orders as a list of links with no accessible name, and `/orders/{order_id}/record` shows one order headed by its customer, with its id and status as paragraphs with no label; `GET /__control/orders` (`TestApp::stored_orders`) gives every stored order ([decision 0473](../decisions/0473-a-capture-drafts-a-list-of-items-of-one-shape-and-a-record-headed-by-its-value-held.md)).
- `/cases/rows-kept` shows the orders in a table with no accessible name, whose headings carry a table library's sort sentence in text hidden to the eye, filled by `POST /api/cases/rows/kept`, whose body adds the `order` the page kept in local storage on every visit after the first ([decision 0474](../decisions/0474-a-capture-drafts-a-table-with-no-name-by-its-headings-as-a-person-reads-them-and-counts-what-the-browser-kept-as-one-shape.md)).

The application stand can stand for a person using it in a window the engine watches and never drives ([decision 0442](../decisions/0442-a-persons-use-is-watched-by-web-use-and-what-it-saw-twice-is-drafted-held.md)): a test sets a walk with `POST /__control/person/walk` (`TestApp::set_person_walk`), and while one is set every page carries a script that takes its steps in order across pages, kept in the tab's session storage: go to a path, wait for a text, linger, type into a form, submit it, send a JSON body by `POST`. `DELETE` on the same path ends it.

The application stand also has a TLS side, behind its feature `tls` ([decision 0157](../decisions/0157-tls-is-switched-on-with-the-provider-stated-in-each-client.md)): it serves the application over TLS and over plain HTTP at once, with one store behind both, so a test asks the plain control address what was stored. A `TestAuthority` made at test time signs a good certificate, one for another name, and one that expired in 2000; a second authority stands for an unknown one. A `TlsFront` puts TLS in front of the MCP stand.

## What Phase 0 delivers

Cases 1 to 4, the control interface, the test harness that asks the application for ground truth, and the conformance suite's launching, locating, and waiting groups run against each spike candidate.
