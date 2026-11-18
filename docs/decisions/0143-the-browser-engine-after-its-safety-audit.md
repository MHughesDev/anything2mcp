# 0143. The browser engine after its safety audit

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the design of [the browser engine crate](../architecture/crates/a2m-pack-web-browser.md) (interface, postures, failures, invariants, open points) and the observing-tab table of [spec section 301](../spec/3-packs/301-the-web-pack.md). Builds on decisions [0110](0110-the-browser-engines-interface-as-building-it-settled-it.md) and [0111](0111-an-observing-tabs-lock-as-built-and-what-the-browser-was-measured-to-do.md), and supersedes two sentences of 0111: "any other child, such as a frame in another process, must take its own before it runs" was not what the code did, and "a lock in the browser was not found" is no longer the whole answer for a tab's moves, downloads, and new windows.

## Context

A reviewer read the crate against its documents and the ten rules and wrote twelve findings without running anything. A thirteenth came from the agent that fixed the vault's redactor. Each was first tried against Chrome for Testing 153, headless, on Linux, with a page of the test application that attempts it, before anything was changed.

## Measured

| Question | Answer |
| --- | --- |
| Can a page move an observing tab by itself: by script, a refresh tag, a form submitted with GET, or to another origin? | Yes, all four reached the test application. So did a `goto` whose answer redirected to another origin |
| Does the browser tell the engine who started a move? | Yes. A move the page asks for is announced first (`Page.frameRequestedNavigation`), one made by `Page.navigate` is not; the start of every move carries a loader, and the held request of its document carries the same loader as its network identifier. A redirect keeps the loader |
| What becomes of the page when the request of its next document is ended? | Ended as aborted, the page stays as it was. Ended as blocked, the browser replaces it with its own error page |
| A move through the tab's history | Is restored from the browser's cache of pages with no request at all |
| Where does a headless browser put a download, with its environment cleared? | In `Downloads` under the temporary folder: outside every checked root. With `Browser.setDownloadBehavior` set to `deny` it writes nothing, and the tab still hears `Page.downloadWillBegin` |
| A frame in another process, as built | Was never let run: it has no document while it waits, the one-off script failed, and the frame was reported as `WORKER`. No socket could open, and no such frame could be observed either |
| Can the socket refusal be placed on each document of such a frame? | Yes: `Page.addScriptToEvaluateOnNewDocument` on the frame's own session, once `Page.enable` has been sent there. Without `Page.enable` it is accepted and does nothing |
| Does `Network.setBlockedURLs` stop a WebSocket? | No |
| Can a page get an unlocked socket, or register a service worker, through a frame it makes for the purpose? | No. The script placed on new documents runs in such a frame before the page can use it |
| Is a service worker of the page's origin attached under the page? | Yes, also one that was already running, and it takes request interception of its own. Its write was failed and its socket refused, as built |
| A window a page opens | Needs a person's gesture, which a tab that only observes never gives. With one, it opens and fetches its page. It is attached by browser-level auto-attach only, and it names its opener even when opened with `noopener` |
| Closing a held window without letting it run | Leaves the page that opened it waiting. Letting it run and then closing it lets a link's request reach the system. Intercepting everything on it, letting it run, and closing it: nothing arrives, and the opener goes on |
| Which session can let a held page run? | Only the one the browser attached it on. The browser's word of that session can come before its answer to the request that made the page |
| What modes does the browser give its own files? | 0600, and 0700 for folders, apart from four files that hold nothing of a session. The cookie value is stored under the fixed key (`v10`) |
| What a headless browser does with a permission request and a file chooser | Asked with a gesture, geolocation, notifications, and clipboard reads were refused with no prompt; a file chooser opened nothing and the page went on. But asked for their state, the clipboard's write, the sensors, the screen lock, payment handlers, storage access, and the background sync and fetch by which a service worker sends requests after its page has gone were `granted`, and camera, microphone, and six more were left to a prompt |
| After `Browser.grantPermissions` with an empty list | Every one of twenty permissions is `denied`, apart from storage access for an embedded page, which stays a prompt that the browser answers alone |
| A hidden tab's request for a location | Is held unanswered until the tab is shown, also with the permission denied; a shown tab is answered at once with the code for a refusal |
| A shared worker an acting tab keeps running | Writes, as an acting tab's page may. An observed page of the same origin cannot reach it: the constructor is refused, also in a frame the page makes |

## Decision

1. **B1, confirmed. A tab is moved only as its posture allows, and the browser holds it there.** Every tab intercepts the request of its own next document. A tab that only observes passes only the request of the `goto` under way and its redirects; a move the page starts is ended before it leaves the browser, as aborted so that the page stays, and reported as `NAVIGATION`. The `goto`'s request is the first, since the `goto` began, that the page did not announce. *Instead of:* comparing the request's address with the address passed to `goto`, which needs the browser's own rewriting of addresses in Rust, and fails closed on every address written another way.
2. **U1, the engine's half. A tab is opened for a set of origins and stays on them.** `acting_tab` and `observing_tab` take `Origins`, made from the destinations the plan declares. A `goto` to another origin is refused before the browser hears of it; a redirect, or a move the page starts, to another origin is ended in the browser; and the helper in the page checks the page's origin in the same call as every locate, read, wait, check, focus, and in-page request, so that a page of another origin that is in the tab all the same is not looked at or acted on. All three are `BrowserError::ForeignOrigin`, `policy`, `surface_not_declared`, and nothing was sent. *Instead of:* a check the driver makes after `goto`, which every acting step can forget, and which comes after the request. The parameter is required so that no caller can open a tab without it.
3. **B2, confirmed. Every download is denied for the whole browser when it starts,** and the tab it began in reports `DOWNLOAD`. A browser that does not take the setting is closed as not owned. Declared downloads, into a checked staging folder, are still to be built. *Instead of:* a download folder inside the profile, which would let any page fill the engine's disk.
   **B2's second part, confirmed: permissions.** Every permission is refused for every origin when the browser starts (`Browser.grantPermissions` with nothing granted), and a browser that does not take it is closed as not owned. The test fails without it: the browser granted a page the clipboard and a service worker's background requests. A file chooser needs nothing: it opens none. *Instead of:* relying on the headless defaults, which grant some permissions and leave others to a prompt that a visible window, still to be built, would show to a person; or denying permissions one by one by name, which a new name would pass.
4. **B3, refuted as stated, and the design corrected.** No frame from another process could open a socket, because none was ever let run. Now such a frame takes its own interception and the socket refusal on every document it loads, and runs. A worker keeps the one-off script: its global is there while it waits.
5. **B4, confirmed for windows, refuted for service workers and for the inner-frame bypass.** The browser attaches every new page before it runs. One with an opener has every request ended, is let run so that its opener is not left waiting, and is closed; the tab that opened it reports `POPUP`, in an acting tab too. One with no opener runs only if the engine made it. The two refuted routes are now pinned by a test.
6. **B5, confirmed for the folder.** The profile folder is set to mode 0700 before the browser starts on it, also one left open earlier, and a folder that is still open to others afterwards is refused as `input`, `path_not_allowed`. On Windows nothing is done: a folder takes the access list of its parent, and the engine reads no access lists.
7. **B6, confirmed.** After the accessibility tree is taken, every node that carries a value is described by the browser itself, which sees into closed shadow roots; a password input found before or after the tree is masked. A text input a page has switched from a password with no `autocomplete` is not recognized: only the redactor knows that value.
8. **B7, confirmed.** A selector that asks about the `value` attribute, or holds an escape that could spell it, is refused as `PasswordField` on a page where a password input carries that attribute. *Instead of:* refusing every such selector, which would take away the usual locator of a radio button.
9. **B8, confirmed.** A snapshot's address, and the address of an in-page response, are cut at the query, and a test holds both. `Loaded.address` keeps it, redacted: the crates above decide by it, and must not show it.
10. **B9, confirmed.** An in-page request is refused for an address outside the tab's origins and follows no redirect: a redirect is `RequestFailed`. The debug forms of `Input` and `PageRequest` print lengths and names, never the text. The fields stay plain text: a secret type there is a change to the crates above.
11. **B10, confirmed.** `BrowserSettings::with_pack_settings` reads the reviewed digests from the pack's settings group under `browser_digests`, and refuses a value it cannot read. The packs must declare the key and call it.
12. **B11, confirmed.** Finding the program and making the folder run on a blocking thread. `availability()` still looks at the disk when called; it is not async.
13. **B12, confirmed.** A request marked safe is matched by origin and by path at a boundary, never by how its text begins.
14. **Redaction comes before any cut** (the thirteenth finding, confirmed: `Page::shown` cut a text to 200 characters and then redacted it). Messages, refused addresses, and the origin taken out of the helper's refusal are redacted whole, then cut; a test lays a known secret across the cut at sixty places. Every other place that cuts, joins, or formats page or browser text was looked at: a read and an in-page answer over the limit are refused, never cut; each name and value of a snapshot is redacted whole before the lines are joined; an address is redacted before its query is cut; the launch messages quote only the browser's protocol message, through the same function. *Instead of:* redacting after the cut as well, which cannot remove the part of a secret the cut left.

## Consequences

- `acting_tab` and `observing_tab` have a new required parameter, and `BrowserError` a new variant. The crates above change with this record.
- A page that sends its user to another origin to sign in is stopped there unless that origin is one the plan declares.
- A page in an observing tab that submits a form stays on screen; before, the browser's error page replaced it.
- A cross-site frame is now observed, where before it stayed empty.

## Not closed

- A move through the tab's history makes no request, so it is not ended. It can only show a page the tab was already sent to.
- If the browser's announcement of a page's move ever arrived after that move's request, and a `goto` were under way, the page's request would be taken for the `goto`'s. It would still be a GET to one of the tab's origins.
- The socket refusal is still a script. A lock in the browser was looked for again and not found.
- A shared worker is refused when the page makes it, and one an acting tab keeps running was not reached. Whether one made past that refusal would be attached was not measured.
- A hidden tab's request for a location waits, unanswered, until the tab is shown. A step that waits on such a page ends at its deadline.
- Refusing every permission also refuses an acting tab's page the clipboard's write: a copy button of an application fails there.
- Windows: the profile folder's access list.
- An acting step's input goes to whatever document has the focus when it arrives. The move in between can only be to one of the tab's origins.

## Revisit if

- The browser stops announcing the moves a page asks for, or stops giving a move's loader on its held request. The test of a page that moves itself fails then.
- A browser version refuses `Browser.setDownloadBehavior`, `Browser.grantPermissions`, or browser-level auto-attach. Every start fails then, by name.
