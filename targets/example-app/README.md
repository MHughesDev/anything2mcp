# The test application

An imaginary order-entry system named `example`. Anything2MCP is proven against this application and never against a real one. The plan for it, and the list of cases it must imitate, is in [docs/plan/test-application.md](../../docs/plan/test-application.md). Cases 1 to 4 are built, and cases 6 to 9, 11, and 12 (the pages under `/cases`), and parts of cases 10, 11, 13 to 15, 17, 23, and 29 to 34; the table under "What is built" in the plan says which parts.

## Running it

```text
cargo run -p example-app              # http://127.0.0.1:4010
cargo run -p example-app -- --port 4020
```

Rust tests do not start the binary. They call `example_app::harness::TestApp::start()`, which starts a new application on a free port inside the test and stops it when the test ends.

## What it serves

| Address | What |
| --- | --- |
| `/orders`, `/orders/{order}` | The list of orders, and one order with its line items |
| `/orders/{order}/items/{item}` | The form for one line item; `/orders/{order}/items/new` is an empty form |
| `GET /api/orders`, `GET /api/orders/{order}` | The same, as JSON |
| `GET`, `PUT /api/orders/{order}/items/{item}` | Read, or create or update, one line item |
| `GET /api/orders/{order}/totals` | Totals for an order. Slow on purpose: case 4 |
| `/orders/{order}/totals` | A page whose own script reads the order's totals from `GET /api/orders/{order}/totals` as it loads, with a made-up anti-forgery header and cookie, and shows the total quantity labelled by its term; it says `Loaded.` once the answer is shown |
| `/orders/{order}/note`, `GET`, `PUT /api/orders/{order}/note` | Case 5: the order's note on a page that holds a fresh anti-forgery token, the same in a meta element `csrf-token`, a hidden input `authenticity_token`, and a cookie `example_xsrf` its script may read. The PUT takes the token once, in the header `X-CSRF-Token` or the body field `authenticity_token` of a JSON or form body; without it, or with one used or never given, it answers 403 (`forgery_token_missing`, `forgery_token_invalid`) and stores nothing. A note is 1 to 200 characters on one line; a refused one is answered 422 quoting the body it was sent, and a token sent in the body is quoted again in capitals in its message. `GET /__control/notes` (`TestApp::stored_notes`) reports each note, its writes, the refusals, and the tokens given and used |
| `GET /api/stock/{item}` | Stock for an item. Every third call since the last reset fails with 503: case 4 |
| `/orders/{order}/delivery` | Case 33: the order's delivery details on a page of the browser's own controls: a select, a check box, a radio group, a date field, and a text area. Save stays disabled until the date is valid, and asks for confirmation in a native dialog before it saves through the JSON interface |
| `GET`, `PUT /api/orders/{order}/delivery` | Read or update the delivery details. Every field and the version are required |
| `GET /api/orders/{order}/handoff`, `GET /api/orders/{order}/pickup` | Case 36: the first answers with a property named `client_secret` (the invented text `example-not-a-secret-0001`) beside ordinary values, and takes an optional query value `access_token`; the second requires a query value `api_key` |
| `GET /api/openapi.json` | Case 30: an OpenAPI 3.1 description of the JSON interface, written by hand and held to the routes by a test |
| `/secure/...` | Cases 13 to 15 and 29: signing in, below |
| `/traps/...` | Case 32: pages that write by themselves, below |
| `/escapes/...` | Case 32, continued: pages that try to get out of a tab that only observes, below |
| `/leaving/{how}/orders/{order}/items/{item}` | Case 32, for the interface driver: the line-item form on a page that moves itself to the other loopback name while it loads (`load`), leaves or opens a window on it after Save (`click`, `window`), shows a refusal once a field is typed (`stale`), or says "Saved." before anything is done (`saved`). Every move aims at `/traps/write/leaving/{how}`. With `loads`, the form loads an image and sends a `fetch` to the other name, at `/traps/write/leaving/loads-image` and `loads-fetch` |
| `/cases/rerender`, `/cases/typed-input`, `/cases/in-page`, `/cases/grid`, `/cases/confirm`, `/cases/notice`, `/cases/frame` | The Phase 1 cases of the interface driver, each a page whose hard part is the page: a list drawn again with new generated identifiers (case 6); an input that keeps only what key presses made (7); an address that moves with no new document, and the page's answer a moment later (8); ten thousand rows of which about fifteen are in the page, and a box "Find row" that brings one in by its key, `R-00001` to `R-10000` (9); a question on some presses in the page's own dialog, then the browser's own on every fourth, the count kept in the browser's page storage (11); a notice that covers a form until it is closed (11); a form in a frame, `/cases/frame/inner`, and a control in an open shadow tree (12). Each page tells the application what happened to it by `POST /cases/events/{name}` |
| `/cases/signed`, `/cases/export`, `/cases/layout/{flag}`, `/cases/locale`, `/cases/note` | Cases 22 and 24 to 27: a page that signs its own request, an export that finishes after a moment and offers a file, one form in two layouts (`a` and `b`, and a third for any other flag), a page that speaks the language the browser says it has, and a note that gives orders to an agent. What each was told is in `GET /__control/cases` |
| `/cases/site-tools` | Package 8.2a: a page that registers tools for agents in the browser: `example_note_save` (tells the application `note-saved: <text>`), `example_total_get` (says it only reads), `example_fail` (tells `failing: now` and throws), and `example_late_tool` (registered 700 ms after the page loads) |
| `/edge/elsewhere`, `/edge/blocked`, `/edge/limited`, `/edge/challenge`, `/edge/maintenance`, `/edge/ok` | Cases 34 and 23: a redirect to the other loopback host name (`localhost` when asked as `127.0.0.1`), a 403 page as a protective edge would send, and a 429 with `Retry-After: 30` |
| `GET /views/xml/orders/{order}`, `GET /views/xml/orders/{order}/items/{item}` | The order and the line item as XML (`application/xml`), in the namespace `urn:example:orders`, written with the prefix `ord` on the order and as the default namespace on its items, each item an element `item` with its code and version as attributes; the line item also carries its `ETag`. 404 as plain text ([decision 0217](../../docs/decisions/0217-the-http-pack-reads-answers-that-are-not-json.md)) |
| `GET /views/xml/declared` | An order as XML whose document type defines an entity built of another: an answer to refuse |
| `GET /views/text/status`, `GET /views/text/legacy` | One status line as `text/plain; charset=utf-8`: the counts of orders and the newest; and another as an old device sends it, in ISO-8859-1, with no character set named |
| `GET /views/bytes/orders/{order}/label` | A small binary label of the order (`application/octet-stream`), made by `views::label` |
| `GET /views/hostile/{name}` | Answers a reader must refuse ([decision 0249](../../docs/decisions/0249-the-http-pack-reads-in-bounded-work-and-sends-nothing-that-may-write-outside-a-live-run.md)): `namespace`, an XML document whose default namespace has an address of 4 MB over 99,999 empty elements (`answers::long_namespace_document`); `control`, an order as XML whose customer holds an escape character as a reference and a bell; `control-text`, a status line with an escape character; `secret`, a binary label that carries `answers::ECHOED_SECRET` |
| `POST /views/search/orders?status={status}` | The orders of one status, as JSON: a search behind `POST` that stores nothing. Any other query is refused with 422 |

Rules a line-item write must pass: the order is a draft; the description is not empty; the quantity is 1 to 9999; the length is a number of inches that is a whole number of sixteenths. An update must carry a version, and a create must carry none: a version left out of an update, or sent with a create, is refused as invalid with 422, like any other field. A version that is present but is not the stored one is a conflict, refused with 409. A refused write stores nothing.

Delivery details: the date is a real date from 2000-01-01 to 2099-12-31, the notes are at most 500 characters, and the version is the stored one (409 otherwise). A submitted order refuses them with 409.

## Signing in

Everything about signing in is under `/secure`, so nothing outside it changes. The credentials are made up and declared as constants in `example_app::harness`: `USERNAME` (`example-user`), `PASSWORD`, `SECOND_FACTOR_CODE`, `BEARER_TOKEN`, `API_KEY` and its header `API_KEY_HEADER` (`x-api-key`), and the cookie names `SESSION_COOKIE` (`example_session`) and `PENDING_COOKIE`.

| Address | What |
| --- | --- |
| `GET`, `POST /secure/sign-in` | The sign-in form: a username and a password field, posted as a form. A right one sets the session cookie (`HttpOnly`, `Path=/secure`) and redirects (303) to the page in the `next` field, which must be under `/secure/`, or to `/secure/orders`. A wrong one answers 401. After `LOCKOUT_AFTER` (5) failures in a row the account is locked until a reset: every sign-in answers 423, the right one too, and checks nothing |
| `GET`, `POST /secure/sign-in/code` | Case 13's one-time code step, when the control interface turns it on. The password step then sets a pending cookie and redirects here instead; the fixed code gives the session. A wrong code answers 401 |
| `POST /secure/sign-out` | Ends the session, clears the cookie, and redirects to the sign-in page |
| `GET /secure/orders` | A page that needs the session. Without one it redirects (303) to `/secure/sign-in?next=/secure/orders`, with `&reason=session_expired` or `&reason=session_revoked` when the session was ended |
| `/secure/api/...` | The whole JSON interface again, for a session cookie. Without a usable session it answers 401 with `error` set to `needs_login`, `session_expired`, or `session_revoked`, and does nothing |
| `/secure/bearer/api/...` | The same, for `Authorization: Bearer` with `BEARER_TOKEN`. Missing: 401 with `WWW-Authenticate: Bearer realm="example"`. Wrong or revoked: 401 with `error="invalid_token"` added |
| `/secure/key/api/...` | The same, for `API_KEY` in the `x-api-key` header. Missing, wrong, or revoked: 401 with `WWW-Authenticate: ApiKey realm="example", header="X-Api-Key"` |

| `GET /secure/sso`, `GET /secure/sso/return` | Single sign-on through an imaginary identity provider, "Example ID", for an application started with `TestApp::start_with_identity_provider()` (decision 0254). The first sends the browser to the provider's `/authorize` with a state it remembers; the second takes the provider's one-use code and the state back once, sets the session cookie, and redirects to `/secure/orders`. A code not issued, one used before, or a state not sent answers 400 and is counted |
| `GET`, `POST /authorize` at `TestApp::provider_url` | The identity provider's own sign-in page, on a second origin, served under the name `localhost` while the application is under `127.0.0.1`, so that each keeps its cookies apart. A right password sets the provider's own cookie and redirects back to the application with a one-use code. With `identity_provider_strays` it redirects instead to its own `/elsewhere` under the name `127.0.0.1`, an origin nobody declared, which counts every arrival |

The sign-in settings (`TestApp::set_auth_settings`) also take `person_at_the_window`, under which the sign-in page and the identity provider's fill in the made-up username and password with a script of their own and send the form, as a person at a window would; `focus_thief`, under which the sign-in page moves the focus from its password field to a note field and the application counts a form whose note holds anything (`typed_elsewhere`); and `foreign_frame`, under which the sign-in page holds a frame from the identity provider's origin. The fault `expire_sessions` ends every session just before the request it applies to is handled.

The harness signs in as a person would with `TestApp::session_cookie()`, which returns the value for a `Cookie` header, passing the code step when it is on.

## Pages that write by themselves

Each sends its write to `/traps/write/{name}`, which counts it under that name; `GET /__control/traps` reports the counts, so a test can show that a discoverer caused none.

| Page | How it writes |
| --- | --- |
| `/traps/on-load` | Its script posts to `on-load` as soon as it loads |
| `/traps/worker` | It starts the worker `/traps/worker.js`, which posts to `worker` |
| `/traps/socket` | It opens a WebSocket to `socket`; the opening handshake is the write, and the upgrade is then refused |
| `/traps/links` | Two links that write when followed with `GET`: one looks like an action by its address (`orders/A-100/delete`), one by its name ("Delete order A-102", at `tool-2`) |

## Pages that try to get out

Each aims at `/traps/write/escapes/{what}`, so the same counts show that none arrived. "The other name" is this application under the other loopback host name, which to a browser is another origin and another site.

| Page | What it tries |
| --- | --- |
| `/escapes/quiet` | Nothing: a page for a test's own script |
| `/escapes/script`, `/escapes/meta`, `/escapes/form` | Once loaded, it moves itself with `GET`: by script, by a refresh tag, by submitting a form |
| `/escapes/form-post`, `/escapes/beacon` | It submits a form with `POST`; it sends a beacon |
| `/escapes/elsewhere`, `/escapes/redirect` | Its script moves it to the other name; its answer redirects there |
| `/escapes/download/{name}` | The answer is a file to save under that name |
| `/escapes/links` | A link that opens a new window, and a link to a download |
| `/escapes/frame` | It embeds `/escapes/inner` from the other name, which opens a WebSocket and sends a `POST` in its first document and in a second one it moves itself to |
| `/escapes/loads` | It loads an image, a script, and a style, sends a `fetch` with `GET` and one with `POST`, and opens a WebSocket, each at `/traps/write/escapes/loads-{image,script,style,fetch,post,socket}` of the origin `from` names (the other name when none), then says "Tried."; with `frame`, it also embeds `{frame}/escapes/loads?from={from}` and says "The frame tried." once that page has (decision 0292) |
| `/escapes/service-worker.js`, `/escapes/shared-worker.js` | Workers that, when reached, send a `POST` and open a WebSocket, and answer with what became of each: the service worker when sent a message, the shared worker when a page connects to it. The service worker also answers a request for `/escapes/answered-by-worker` itself, which the application never serves |

## The control interface

Under `/__control`. It exists only in this application, and no fault ever applies to it.

| Call | What |
| --- | --- |
| `POST /__control/reset` | Restore the seed data; clear faults, counters, and settings |
| `GET /__control/orders/{order}` and `.../items/{item}` | Exactly what is stored, or `null` |
| `GET /__control/writes` | Every write since the last reset: which request changed which record |
| `GET`, `POST`, `DELETE /__control/faults` | List the faults, switch one on, switch all off |
| `POST /__control/settings` | How long the slow endpoint waits |
| `GET /__control/orders/{order}/delivery` | Exactly what is stored for the delivery details, or `null`, and how many times they were written |
| `GET /__control/auth` | What was counted about signing in: attempts, sign-ins, failed sign-ins and codes, whether the account is locked, sign-outs, usable sessions, requests refused for a session, a wrong token, or a wrong key, and which credentials are revoked |
| `POST /__control/auth/settings` | Sign-in modes: `second_factor` (case 13) and `single_session`, where a new sign-in revokes the others (case 15) |
| `POST /__control/auth/sessions/expire`, `.../revoke` | End every usable session, as timed out or as ended by an administrator (case 14) |
| `POST /__control/auth/revoke` | Revoke `bearer_token` or `api_key` until a reset |
| `GET /__control/traps` | How many writes each trap page caused, by name |
| `POST /__control/shipment` | Case 35: how `GET /api/orders/{order}/shipment` answers its optional `carrier` object until a reset: `{"answer": "object"}`, `"null"`, `"missing"`, or `"text"` (a wrong kind). The description gives `carrier` as neither required nor nullable. Case 37: the same switch answers the hand-off's optional `packages` as `null` or leaves it out, and with `"required_null"` its required `carrier_code` as `null` |
| `GET /__control/cases` | What the pages of cases 6 to 9, 11, and 12 told the application since the last reset, oldest first, each as `name: text` (`TestApp::case_events`) |
| `GET /__control/seen` | The method and path, without the query, of every request the application received outside the control interface since the last reset, oldest first and at most 10,000, so a test can show that a refused value sent none |

A fault is a delay, a status, "lost after commit", "redirect after commit", or "changed shape", for the next request, a number of requests, or until cleared, on every path or on paths with a given start. "Lost after commit" (case 17) carries a request out, so its write is stored, then closes the connection without any answer; it applies only to methods other than `GET` and `HEAD`, so a read does not use it up, and it needs the application served by `example_app::serve` or the harness. "Changed shape" (case 31) renames one field wherever it appears in a JSON answer. "Redirect after commit" carries a request that may write out, so its write is stored, then answers `303 See Other` with the `Location` the fault gives, in place of the application's own answer: a write that was made and whose answer is not the application's verdict.

## Seed data

Order `A-100`, a draft, with line items `L-101`, `L-102`, and `L-103`. Order `A-101`, submitted, with `L-201`. Order `A-102`, a draft with no line items. Every order has delivery details at version 1: standard shipping, no gift wrap, normal priority, no notes. No one is signed in.
