# The test application and the driver conformance suite

Anything2MCP is never tested against a real business application. Everything is proven against one purpose-built application that imitates the hard cases, and against a fixed list of behaviors every interface driver must pass. Phase 0 builds the first part of both; later phases add to them.

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
| 10 | Uses custom controls: a filtering dropdown that loads options late, a date picker, a rich-text editor, a file input, a toggle, a tree | One widget driver each | 1, then 8 |
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
| 22 | Computes a value in the browser before saving, and signs a request in the browser | Endpoint mining stopping at shadow; the HTTP binding marked unsupported | 4 |
| 23 | Answers with a rate-limit status, a challenge page, or a maintenance page when told to | `edge_blocked` and surface health; no retry | 4 |
| 24 | Starts an export that finishes later and offers a download | Long-running jobs and downloads | 4 |
| 25 | Shows one of two layouts depending on a flag | Ordered locators; a new variant stops | 8 |
| 26 | Switches language and number format | Fixed locale; comparison by code, not label | 8 |
| 27 | Returns text containing instructions aimed at an agent | Untrusted fields; the manipulation test suite | 3 |
| 28 | Exists as a small native desktop program with the same data | The desktop driver | 8 |

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
| Speed | Each budgeted path in spec section 9, measured |

Results are kept as a table per implementation and operating system. The Phase 0 spike fills in the first rows for each candidate base; choosing a base means comparing those tables.

## What is built

As of slice 0.3, on 2026-11-16: cases 1 to 4, in `targets/example-app/`. Its [README](../../targets/example-app/README.md) lists every address, the rules a write must pass, the control interface, and the seed data.

- **The control interface** has Reset, Inspect, Count writes, and Set a fault. Set a mode is not built; the first case that needs it is case 13.
- **Faults** so far are a delay and a status. Dropping a connection after a commit arrives with case 17.
- **The harness** is `example_app::harness::TestApp`. A test starts an application on a free port, uses it over HTTP, and asks the control interface what was stored and how many times. Two applications in one test share nothing.
- **Lengths** are stored as whole sixteenths of an inch and travel as decimal text, so the application itself never holds a float.
- **A line item that does not exist** has no form at its own address; the form for a new one is at a different address. That is the shape hypothesis H-23 is about.
- The form was driven once by hand in a real browser, located by role and label: an update, a second save, a validation message, and a create all behaved. No automated test covers the page's script yet; the first one comes with the interface driver.
- The product's crates may use the application in their tests only. `tools/docs_audit.py` refuses it as an ordinary dependency.

## What Phase 0 delivers

Cases 1 to 4, the control interface, the test harness that asks the application for ground truth, and the conformance suite's launching, locating, and waiting groups run against each spike candidate.
