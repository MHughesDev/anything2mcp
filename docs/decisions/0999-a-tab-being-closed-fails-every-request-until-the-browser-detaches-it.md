# 0999. A tab being closed fails every request until the browser detaches it, and its page's handlers are erased first

**Status:** Accepted. Design decision, made by the worker who traced a failure of the test of [decision 0292](0292-a-web-tab-sends-only-to-its-surface-and-the-destinations-its-tool-declares.md) on Windows, under the lead's brief. The owner had not read this record when it was written. It keeps the promise of 0292 and of [section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md): nothing is sent to an origin the tool does not declare. It adds one: a tab being closed sends nothing more, to any origin.

**Date:** 2026-11-25

**Changes:** [`a2m-pack-web-browser`](../architecture/crates/a2m-pack-web-browser.md): how a tab is released, how a hung tab is closed, and `OpenBrowser::closing`. It leaves every verdict on a request of an open tab, every posture, and the drivers above as they were.

## Context

In one Windows run, the test of 0292 found a request at the stand's trap on the application's other name after the second read, which the tool did not declare; the check after the first read had passed. The first read's tab was the source.

A tab was released by ending its subscription to its events, then asking the browser to close its page. A request of the page that the browser had paused for the tab and the tab had not yet answered was then never answered. The engine's comment said such a request stays paused and is never sent. That is false. When a tab's session is detached, which closing its page does, the browser continues every request it still holds for the session unchanged: Chromium's `InterceptionJob::Detach` calls `InnerContinueRequest` with no change. Measured on Chrome for Testing 153 on Linux: a failure sent for a held request after the page's close reached it is refused with "Session with given id not found", and the request reaches the stand.

A page also makes requests while it is closed. A `fetch` with `keepalive` from a `pagehide` handler, added to the stand's page, reached the undeclared origin in 3 runs of 3 with the old release. A load of the page still on its way when the step ended does the same, more rarely: on Linux under load, 0 runs of 30; on Windows, once.

## Decision

1. **A released tab hears its events until the browser detaches it.** From its release on, every request its page, its frames, or its workers make is failed before it leaves the browser, whatever its origin and method, and is not reported: nobody reads the tab any more. A worker or a frame that attaches while the tab is being closed is left waiting, and never runs. The subscription ends at the browser's word that the session is detached, or at the deadline of a call.
2. **The page's handlers are erased before it is closed.** The tab's document is replaced with an empty one the engine writes, with no request (`Page.setDocumentContent`). Replacing a document erases every handler the page set on its window and its document, so the page's leaving runs nothing of the page's. Only then is the page closed.
3. **A hung tab is closed the same way** as far as a hung page allows: while its page and windows are closed, every request it still makes is failed until its session ends. Its script has been ended first, as before; its document is not replaced, since a hung page answers nothing.
4. **The browser says how many of its tabs are being closed** (`OpenBrowser::closing`): released, and not yet detached. A test that asks a stand what arrived waits until it is zero.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Hear the tab until it is detached, and nothing more | A handler of the page's leaving runs as the page closes, and the browser detaches the session before the tab's answer arrives: 9 of 10 runs leaked |
| Send the page to `about:blank` first, and close it once that document has loaded | The old document runs its leaving after the new one has loaded, and outlives the wait: 1 of 5 runs leaked |
| Block every address in the page first (`Network.setBlockedURLs`), or switch its scripts off (`Emulation.setScriptExecutionDisabled`) | Added before `about:blank`, neither stopped the request of a `pagehide` handler in the leaving document: 8 and 4 of 15 runs leaked |
| Pause the page's scripts in the debugger before closing it | No request arrived before the checks, and the handler's request then arrived once the session was detached and the scripts resumed: a later leak, not none |
| A second gate on the browser's own session, through which every request passes after its tab's | It caught every request a closed tab's session let go (0 of 10 leaked). It would also hold every `fetch` of a sign-in tab, whose body carries the password the person types into the engine's process, which [decision 0254](0254-a-person-signs-in-in-a-window-of-the-engines-own-browser-and-the-web-pack-stops-on-every-declared-sign-of-expiry.md) forbids; and it pauses every request of every tab twice. Kept as the next step if a request is ever seen to leave a closing tab again |

## Consequences

- With the erasure, the stand's page with a `pagehide` handler leaked in none of 12 runs, and in none of 30 on Linux under load; its request did not arrive later either, also where the tool declares its origin.
- A page's handler of its leaving never runs when the engine closes its tab. A system that relies on one to save what a person did is not served by it; no read or write the engine makes is.
- What still could leave: a request a page's timer or a still-running task makes in the moment between the erasure and the detach, and the browser announces after the detach. None was seen. The gate on the browser's own session above would close it.
- A released tab now lives until its session is detached, within the deadline of a call; `OpenBrowser::tabs` counts it out at its release, as before.

## What would reopen it

A request seen at an undeclared origin from a tab being closed; a browser version that answers a held request differently at detach; a way to close a page without running its handlers.
