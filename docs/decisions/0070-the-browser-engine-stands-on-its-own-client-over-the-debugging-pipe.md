# 0070. The browser engine stands on its own client, over the debugging pipe

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the choice [decision 0004](0004-rust-browser-engine-on-the-devtools-protocol.md) left to a spike; the first open point of the [browser engine's design](../architecture/crates/a2m-pack-web-browser.md); research brief 1. Roadmap package 0.4.

## Context

Decision 0004 chose a Rust engine on the DevTools protocol and left its base to a spike: an existing crate, or a client of our own. The design since added two conditions. The browser's protocol must travel on an inherited pipe, not a port ([section 204](../spec/2-kernel/204-supervising-subsystems.md)). And the base stays behind a private trait, so that it can change without touching the crates above.

The spike ran a client of our own, the best existing DevTools crate, and a WebDriver client against the test application, and checked six more crates by their dependency trees and source. [Research note 17](../research/17-browser-engine-base.md) has the table, the measurements, and what was run against what was only read.

## Decision

**The base is a DevTools-protocol client of our own, inside `a2m-pack-web-browser`, over the debugging pipe.** No outside DevTools or WebDriver crate.

- **The client.** One reader task, one writer, requests matched to replies by number, events sent to subscribers, one session per tab on the one connection. A message is JSON ended by a NUL byte. A message that is too large, is not a protocol message, or answers no request closes the link, and every pending request fails. Each failure says whether the request had been written, which is what `Dispatch` reports.
- **The protocol's commands are written by hand.** One private module holds a typed request and reply for each command the engine uses, and nothing outside it sees untyped JSON. *Instead of:* a crate of types generated for the whole protocol, which cost 55 seconds of build for the few dozen commands needed.
- **Locating goes through the helper first.** A locate by an injected helper took 0.2 ms; the protocol's accessibility query took 16.7 ms every time. The accessibility tree stays the source for snapshots, and the check on what the helper found.

**The pipe, on each operating system.** Starting a process is the supervisor's, so this code belongs to `a2m-provider`, and the pack only states how its program takes the pipe.

| | How the browser is given the pipe | With what | Proven |
| --- | --- | --- | --- |
| Linux, macOS | `--remote-debugging-pipe`; the two pipe ends are placed on file descriptors 3 and 4 between fork and exec | The `command-fds` crate. The `unsafe` is inside it; none is ours | Run on Linux. Compiled for macOS; the same code path on both |
| Windows | `--remote-debugging-pipe` and `--remote-debugging-io-pipes=<read>,<write>`, naming two inherited handles by value | Two named pipes made with Tokio, whose client ends are opened as blocking handles and marked inheritable by one `unsafe` call to `SetHandleInformation` through `windows-sys`. The call carries a comment saying why it is sound, and a test on Windows | Compiled only. Not run |

On Windows the mark is set, the browser started, and the mark cleared while holding a lock that every process start in the engine takes, because the standard library starts each child with handle inheritance on, and a second child started at that moment would otherwise inherit the browser's pipe.

*Instead of, on POSIX:* a shell that moves its standard input and output to descriptors 3 and 4 and then becomes the browser. It needs no `unsafe` anywhere and ran, but the supervisor would start a shell and not the program it identified by digest. *And instead of:* our own code between fork and exec, which is `unsafe` of ours and must handle two descriptors that collide, as the helper crate already does.

**Finding a browser.** In this order, and the first found is used:

1. A program a person named in settings.
2. A browser installed by the explicit action `browser.install`.
3. A fixed list of the places where the stable Chrome, Edge, and Chromium are installed on each operating system, kept as data.

Never a search of the path, never an environment variable outside the settings layers, and never a download at first use or during a run. With none found the failure is `busy`, `browser_unavailable`, and its next step names the install action. Whatever browser is found, only its program is used: every launch passes a data directory the engine owns, and the browser's own profile is never named, read, or copied.

**The launch arguments are a fixed list in the crate.** It never contains `--no-sandbox`, and never a flag that hides automation. A test holds both.

**What CI needs.** The hosted Linux, Windows, and macOS machines each carry Chrome and Edge, so the tests download nothing and find the browser by the same fixed list. A browser test that finds no browser fails; it does not pass by skipping. The first browser test on the Windows machine is the proof of the Windows handoff, which nothing has yet run. If the browser's sandbox will not start on the hosted Linux machine, the test harness alone may switch it off through a setting that exists only for tests, and the journal says so.

## Why

| | Our own client | The best existing crate, `chromiumoxide` |
| --- | --- | --- |
| The pipe | Yes | No: a loopback port and a WebSocket |
| Our code for the protocol and the launch | 166 lines and 163 lines | None |
| Crates added | 7 | 44 |
| Build time added | 3 s | 111 s |
| Start to first value; a read on a warm page | 0.35 to 0.40 s; 0.2 ms | The same |
| A wait for an element; a locate by role and name | Ours to write | Ours to write: the crate has neither |
| A browser killed during a call | Fails in 20 ms, saying whether the request was sent | Fails in 10 ms, not saying |

The crate saves the small part. Every other crate found fails a rule before size matters: a licence outside the closed list, a default that hides automation, usage data sent by default, a browser fetched silently, or no maintenance. WebDriver needs a driver program matched to the browser's major version, which would have to be fetched whenever the browser updates; the one on the spike's machine refused the browser there.

## Consequences

- `a2m-pack-web-browser` depends on `tokio`, `serde`, and `serde_json` from outside, and on nothing else.
- `a2m-provider` gains `command-fds` on POSIX and a direct use of `windows-sys` on Windows, with one `unsafe` call that the workspace's lint is told to allow at that one place.
- The supervisor's `Foreign` link must carry how the child receives its pipe: two descriptor numbers on POSIX, and on Windows an argument into which the two handle values are written. Its design sketch does not say this yet.
- The waiting, locating, and actionability logic is all ours, as decision 0004 already accepted.
- H-01 is supported in part: a direct client works, is small, and meets the browser budgets on Linux. It was not compared with a sidecar. H-03 is supported on a late element: an observer notices it at once, and a 50 ms poll up to 50 ms late.

## Revisit if

- The Windows handoff cannot be made to work on the hosted machine. Then Windows alone uses a loopback port, with the random address the browser writes into its data directory as the secret, as section 204 allows where the platform leaves no choice; or decision 0004's fallback applies.
- An existing crate gains the pipe on all three operating systems and drops the defaults that rule it out. The conformance suite then compares the two behind the private trait.
- The hand-written commands pass about eighty, or a wrong field escapes the tests more than once. Then generated types on our own transport.
- A 30-minute run, or a memory figure, misses the budgets of [section 225](../spec/2-kernel/225-performance.md). Neither has been measured.
- `command-fds` stops being maintained. About twenty lines of our own, with a commented `unsafe` and a test, replace it.

## Not proven

Nothing ran on Windows or macOS. No program ran for more than a few seconds. The first browser version that takes `--remote-debugging-io-pipes` is not known, so no minimum version is set yet.
