# The test application

An imaginary order-entry system named `example`. Anything2MCP is proven against this application and never against a real one. The plan for it, and the list of cases it must imitate, is in [docs/plan/test-application.md](../../docs/plan/test-application.md). Cases 1 to 4 are built, and parts of cases 10, 11, 13 to 15, 17, 23, and 29 to 34; the table under "What is built" in the plan says which parts.

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
| `GET /api/stock/{item}` | Stock for an item. Every third call since the last reset fails with 503: case 4 |
| `/orders/{order}/delivery` | Case 33: the order's delivery details on a page of the browser's own controls: a select, a check box, a radio group, a date field, and a text area. Save stays disabled until the date is valid, and asks for confirmation in a native dialog before it saves through the JSON interface |
| `GET`, `PUT /api/orders/{order}/delivery` | Read or update the delivery details. Every field and the version are required |
| `GET /api/openapi.json` | Case 30: an OpenAPI 3.1 description of the JSON interface, written by hand and held to the routes by a test |
| `/secure/...` | Cases 13 to 15 and 29: signing in, below |
| `/traps/...` | Case 32: pages that write by themselves, below |
| `/escapes/...` | Case 32, continued: pages that try to get out of a tab that only observes, below |
| `/edge/elsewhere`, `/edge/blocked`, `/edge/limited` | Cases 34 and 23: a redirect to the other loopback host name (`localhost` when asked as `127.0.0.1`), a 403 page as a protective edge would send, and a 429 with `Retry-After: 30` |

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
| `/escapes/service-worker.js`, `/escapes/shared-worker.js` | Workers that, when reached, send a `POST` and open a WebSocket, and answer with what became of each: the service worker when sent a message, the shared worker when a page connects to it |

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

A fault is a delay, a status, "lost after commit", "redirect after commit", or "changed shape", for the next request, a number of requests, or until cleared, on every path or on paths with a given start. "Lost after commit" (case 17) carries a request out, so its write is stored, then closes the connection without any answer; it applies only to methods other than `GET` and `HEAD`, so a read does not use it up, and it needs the application served by `example_app::serve` or the harness. "Changed shape" (case 31) renames one field wherever it appears in a JSON answer. "Redirect after commit" carries a request that may write out, so its write is stored, then answers `303 See Other` with the `Location` the fault gives, in place of the application's own answer: a write that was made and whose answer is not the application's verdict.

## Seed data

Order `A-100`, a draft, with line items `L-101`, `L-102`, and `L-103`. Order `A-101`, submitted, with `L-201`. Order `A-102`, a draft with no line items. Every order has delivery details at version 1: standard shipping, no gift wrap, normal priority, no notes. No one is signed in.
