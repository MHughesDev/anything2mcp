# 0110. The browser engine's interface as building it settled it

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the interface sketch in the design of [the browser engine crate](../architecture/crates/a2m-pack-web-browser.md), where the code differs. Builds on [decision 0070](0070-the-browser-engine-stands-on-its-own-client-over-the-debugging-pipe.md).

## Decision

1. **`Browsers` is built from a pack's context, inside `Pack::register`.** The sketch had the product crate build it. A `PackContext` exists only while a pack registers ([decision 0089](0089-the-provider-contract-as-building-it-settled-it.md)), so the pack crate built on this one calls `Browsers::new` there and keeps it. Its manifest declares the root `profiles` under `write_roots` and the paths of `KNOWN_PLACES` under a requirement.
2. **A test names its browser through a feature that the product never has.** The module `test_support`, behind the feature `test-support`, reads two variables: `A2M_TEST_BROWSER`, the program's absolute path, and `A2M_TEST_BROWSER_NO_SANDBOX=1`, which adds `--no-sandbox`. The setting that carries the second exists only in a test build. *Instead of:* a public setting for extra arguments, which the product could then be told to use; *and instead of:* a variable read by the library, which decision 0070 forbids. A crate above asks for the feature in its dev-dependencies only.
3. **Errors.** Every `BrowserError` answers `dispatch()`, so the sketch's `Result<Dispatch, _>` became `Result<(), _>`. `failure()` gives a `Failure` only for errors that mean the same whatever the step, and `None` for the rest. A method that only looks always says `NotSent`. The supervisor's own failure is kept whole, so a program the manifest does not declare is `policy`, `effect_not_declared`, not `path_not_allowed` as the sketch had it. New: `NotLoaded`, `PasswordField`, `TooLarge`, `RequestFailed`, `Unexpected`; `unknown_variant`, `browser_not_owned` for a program that is not a browser.
4. **`Look` returns futures that are `Send`,** written as `impl Future` in the trait, so no macro and no boxing. `tree` and `network` are left out until they are built; `check` is part of `input`.
5. **`locate` does not wait, and only rendered elements match.** A step that may be early waits for `Visible`. *Instead of:* a locate that waits to its deadline, which hides a missing wait and makes "not there" cost the whole deadline.
6. **A wait is re-armed when the document is replaced under it;** an input and an in-page request are never sent again.
7. **One writer task.** A request is queued, and written only by that task, so a dropped call cannot leave half a message on the pipe, and one that was abandoned in the queue is never written. *Instead of:* a lock around the writer held by each call, as the spike had, where a cancelled call corrupts the stream.
8. **Events go to one subscriber per session,** in order, on a channel that drops nothing. *Instead of:* one broadcast to all, where a slow reader loses an intercepted request.
9. **The data directory is `profiles/<target>/<environment>/<kind>-<digest of the identity>`.** The sketch had `profiles/<identity>/`; an identity is free text and two targets may share one. [The technical reference](../reference/technical-reference.md) still shows the short form.
10. **The fixed launch arguments** add `--enable-automation`, the switches that stop the browser's own background traffic, and `--password-store=basic` with `--use-mock-keychain`. The last two keep the engine from asking the operating system's key store, where a person's own browser keeps its key. The cost: cookies in the data directory are not encrypted by a key outside it.
11. **`Close::Erase` is left out.** A checked path can remove a file and cannot remove a folder.
12. **Snapshots mask a password by node, not by value.** The helper lists the page's password inputs as objects, the engine asks the browser for the number of each, and the builder shows a mask for those nodes. No password value is read to do it.

## Not proven

Nothing ran on Windows or macOS. The cross-target lint cannot run for this crate: its tree compiles C, through the store that `a2m-provider` reaches. No budget was measured.
