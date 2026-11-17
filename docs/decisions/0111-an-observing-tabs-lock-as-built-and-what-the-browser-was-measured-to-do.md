# 0111. An observing tab's lock as built, and what the browser was measured to do

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the table "In an observing tab" in [spec section 301](../spec/3-packs/301-the-web-pack.md) and in the design of [the browser engine crate](../architecture/crates/a2m-pack-web-browser.md).

## Context

The design said the interception of an observing tab "is attached to every worker before it runs", and that a WebSocket is "refused at creation". Building it against Chrome for Testing 153, headless, on Linux showed what the browser offers.

## Measured

| Question | Answer |
| --- | --- |
| Does a dedicated worker take request interception of its own? | No. The protocol answers that the method is not found on the worker's session |
| Does the page's interception see a dedicated worker's requests? | Yes. A write sent by a worker was paused on the page's session and failed there; the test application stored nothing. From an acting tab the same worker's write was stored |
| Can a worker be held until it is locked? | Yes. It attaches waiting, and runs when told |
| Does request interception see a WebSocket? | No |

## Decision

- **Requests:** intercepted on the page's session before the page can run. A dedicated worker relies on that interception. Any other child, such as a frame in another process, must take its own before it runs.
- **Fail closed.** A child that cannot be locked is not let run, and is reported. An intercepted request whose answer is lost stays paused.
- **Sockets, peer connections, shared workers, and service workers** are refused by a script placed in the page and in each worker before it runs, and each refusal is reported. *Instead of:* leaving them open and reporting a limitation, which lets a page write over a socket while it is observed. This lock is weaker than the interception: a page that sets out to defeat it can. A lock in the browser was not found.
- **What is reported** is a method, or a word for what was refused, and the address without its query.

## Revisit if

- A browser version stops routing a dedicated worker's requests through its page. The test that writes from a worker fails then.
- The protocol gains a way to refuse a socket in the browser.
