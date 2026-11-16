# The test application

An imaginary order-entry system named `example`. Web2MCP is proven against this application and never against a real one. The plan for it, and the list of cases it must imitate, is in [docs/plan/test-application.md](../../docs/plan/test-application.md). Cases 1 to 4 are built.

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

Rules a write must pass: the order is a draft; the description is not empty; the quantity is 1 to 9999; the length is a number of inches that is a whole number of sixteenths; an update carries the version it read, and a create carries none. A refused write stores nothing.

## The control interface

Under `/__control`. It exists only in this application, and no fault ever applies to it.

| Call | What |
| --- | --- |
| `POST /__control/reset` | Restore the seed data; clear faults, counters, and settings |
| `GET /__control/orders/{order}` and `.../items/{item}` | Exactly what is stored, or `null` |
| `GET /__control/writes` | Every write since the last reset: which request changed which record |
| `GET`, `POST`, `DELETE /__control/faults` | List the faults, switch one on, switch all off |
| `POST /__control/settings` | How long the slow endpoint waits |

A fault is a delay or a status, for the next request, a number of requests, or until cleared, on every path or on paths with a given start.

## Seed data

Order `A-100`, a draft, with line items `L-101`, `L-102`, and `L-103`. Order `A-101`, submitted, with `L-201`. Order `A-102`, a draft with no line items.
