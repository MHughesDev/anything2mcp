# 0117. The local channel and the person surface on one machine, as built

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the interface sketch and the open points of the design of [`a2m-local`](../architecture/crates/a2m-local.md). It answers the part of research brief 26 that the channel needs, and none of brief 27.

## Context

The design of `a2m-local` gave the channel, the lifecycle, and the person surface as a sketch, and left the crates for the pipe, the peer checks, and the locks to research brief 26, and the presence prompts to brief 27. Building the channel, the engine lock, and the approval page settled the points below. Only Linux could be run here.

## Decision

**Outside crates (brief 26, as far as it was needed).**

1. **No new crate enters the tree.** The socket and the named pipe are Tokio's own (`tokio::net::UnixListener`, `tokio::net::windows::named_pipe`). The peer's user on a socket is Tokio's `peer_cred`. This process's user id is `rustix`, already in the workspace. The Windows calls are `windows-sys`, already in the workspace, with four more of its features. The file locks are the standard library's `File::try_lock`. Rejected: a crate that wraps sockets and pipes behind one type, because the two differ exactly where the security checks are, and a file-lock crate, because the standard library now has the call.
2. **The engine end is served by `axum::serve` over a listener of our own;** the client end is `hyper`'s HTTP/1.1 connection (features `client`, `http1`) with `hyper-util`'s adapter between Tokio's and hyper's stream traits (feature `tokio`). Both crates were already in the tree through axum; they are now named in the workspace. Rejected: a client of our own for HTTP/1.1, which is more code than the two lines that name hyper.

**The channel.**

3. **The service is an `axum::Router`.** `Channel::serve` takes the router the product crate composed. The channel puts a `Peer` (user, process, client label) into the extensions of every request. A front door reads it and hands `Peer::credential()` to the kernel's one verifier; the channel checks who connected and decides nothing else.
4. **The hello is a request the channel answers itself:** `GET /local/hello` with the client's version, the local API version, and the client label in three headers. Any other request on a connection that has not said hello is refused with `input`/`hello_invalid` and does not reach the service. A different local API version is `input`/`local_api_mismatch`. The local API version is 1.
5. **The engine lock is two file locks, taken together:** `engine.lock` in the runtime directory and `engine.lock` in the data directory. The design had one, in the runtime directory. Two runtime directories can lead to one data directory, for example a shell with no runtime directory of its own beside one that has it, and two engines on one store must never run.
6. **The engine and the client name the user as text:** the user id on macOS and Linux, the security identifier on Windows. The product crate gives the same text to `KernelParts::os_user`.
7. **On Windows the pipe's name carries the security identifier and sixteen hexadecimal characters of the digest of the runtime directory's path,** so that two homes on one account do not share a pipe. Its access list is protected and has one entry, the user. The engine reads the client's user from the token of the client's process, found by `GetNamedPipeClientProcessId`, because impersonating the client needs a byte to have been read first. The client reads the engine's the same way.
8. **A client's request ends when its connection ends.** The link watches the task that drives the connection and answers `transient`/`connection_lost` when that task has ended, instead of trusting the HTTP library alone to wake a request whose connection the engine closed. One run of the tests on a loaded machine did not end, in the test where the engine closes a connection unread. The process then held one engine's lock and its listener and no connection, which fits a request left waiting on a connection that was gone. The cause was not reproduced: nine thousand closed connections without the guard all ended. The guard is right whatever the cause, and each wait in the channel's tests now fails by name after a minute instead of waiting for ever.

**The person surface.**

9. **The page is plain HTML embedded in the crate, with no script.** Spec section 503 names TypeScript in `web/console` for the diff page. Until that exists the local page is two small files with inline style, a content policy that loads nothing, and two forms.
10. **Two one-time values.** The address holds a code that names the page; it goes into the run's result and may reach an agent. Each time the page is served it carries a fresh value in its forms, and a press must bring the value of the page served last. A press takes the value whether it fits or not.
11. **The approval is for the change the page showed.** At a press the desk is asked again, and nothing is recorded unless the run still waits with the hash the page showed. The prompt is raised only after that.
12. **The presence prompt is an interface, `PresencePrompt`, and the crate ships one implementation that says none is available.** With it a press of Approve records nothing, and `confirm_presence` fails with `policy`/`presence_failed`, so no protected setting is loosened. A prompt answers one of four ways: present, confirmed in a dialog that proves nobody, dismissed, not available. Only the first two record an approval, and only the first gives proof of presence. Rejected: recording an approval with the check `not_available` when no prompt and no dialog exists, because the link is in the agent's hands and a script could then approve.
13. **Every answer on one machine is recorded with the channel `local_prompt`.**
14. **An answered change gets no second page.** A run still rests at `needs_approval` after its approval until it is resumed, and is looked at meanwhile. The surface remembers the last 256 answered changes in memory and answers an ask about one with words and no link. Without this a second page opened for a change already approved, and the listener stayed open for the whole window.

## Not built, and what CI must show

- Start on demand, the start lock, the drain and restart of an older engine, `--in-process`'s own command, and the `engine.*` actions of the client end.
- The operating systems' presence prompts and the fallback dialog (brief 27 is not run), notifications, the sign-in status page, and `ask_session`, which answers that the person cannot be reached.
- The stored diff on the page: `ApprovalDesk` gives no way to read it. The page shows the tool, the run, who started it, and the hash.
- The Windows code was compiled and linted for `x86_64-pc-windows-msvc`, and the macOS code for `aarch64-apple-darwin`, through a crate that holds only the operating-system module, because the whole crate's tree compiles SQLite. Nothing was run on either. CI must show the Windows tests in `os/windows.rs` and the channel tests passing on Windows and macOS. One case is unknown: whether a client running elevated can be read by an engine that is not.

## Consequences

- Spec section 222 and the design say the hello is the first request and the engine answers with its version; that is what is built. They say one lock in the runtime directory; the design is corrected, and section 222's sentence is owed a small edit.
- A decline, by a press or by silence, is recorded by the policy engine, and the run still rests at `needs_approval`: nothing below this crate ends it as `expired`. That is the kernel's to fix. Until then the surface's memory keeps a declined change from being asked about again, for as long as the engine runs.
- `run.resume` now and then answers `needs_approval` for an approved run, in the moment between the runner's claim of the job and its work. The surface's test resumes again, as a caller would. That too is the kernel's to fix.
