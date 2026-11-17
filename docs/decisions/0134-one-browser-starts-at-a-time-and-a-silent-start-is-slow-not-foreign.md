# 0134. One browser starts at a time, and a silent start is slow, not foreign

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the design of [the browser engine crate](../architecture/crates/a2m-pack-web-browser.md), and the failure table of [spec section 301](../spec/3-packs/301-the-web-pack.md). Builds on decisions [0070](0070-the-browser-engine-stands-on-its-own-client-over-the-debugging-pipe.md) and [0110](0110-the-browser-engines-interface-as-building-it-settled-it.md).

## Context

The first run on a hosted Windows machine that got as far as a browser's first answer failed four tests in the same way: the browser was started, the first request was written, the pipe stayed open, and nothing answered in the 30 seconds then allowed. The log's timings say why.

| Browsers started at the same moment | Time to a first answer |
| --- | --- |
| The first four on the machine | More than 30 s each; all four failed at the deadline |
| The next four | Just under 30 s each; all passed |
| Three, about a minute later | Under 9 s for the whole tests |
| Four, after that | 5 to 9 s for the whole tests |

The machine was cold, and four browsers were starting against each other. A handle leaked between starts was ruled out: it would hold a dead browser's pipe open, and could not make a live one slow and then succeed; and every start in those tests was under the supervisor's start lock.

The same repeated runs showed a second fault, on Linux, one run in nine: a call made right after `answer_dialog` was told a dialog was still open.

## Decision

1. **One browser starts at a time in a process,** from the start of the program to its first answer, whichever `Browsers` asks. The product has one `Browsers`, so this changes nothing there; it holds also where tests, or two packs, each have their own. *Instead of:* leaving starts in parallel and only raising the deadline, which keeps the contention that made each start slow.
2. **The launch deadline is 120 seconds by default, and it is a ceiling, not a budget.** The budget for a cold start is one second ([section 225](../spec/2-kernel/225-performance.md)), and a warm machine meets the deadline by a wide margin. The ceiling only decides when the engine gives up on a machine that is cold: after it came up, after the browser was updated, or while the system's scanner reads the program for the first time. The one figure measured is "over 30 seconds with four at once"; how long one cold start alone takes on that machine is not known, so the ceiling is set well above what was seen. It is a setting, `launch_deadline`. *Instead of:* 30 seconds, which a real machine exceeded.
3. **A start that is silent with its pipe still open is `transient`, `timeout`,** as `LaunchReason::NoAnswer`. Silence proves only slowness. `unknown_variant`, `browser_not_owned` stays for what does prove the program is not a browser the engine can own: its pipe ended, it sent something that is not the protocol, it speaks another protocol version, or its digest is not a known one. Either way the program is closed, and nothing has reached any system, so trying again is safe. *Instead of:* calling every unanswered start "not a browser", which told a person with a slow machine to check a program that was fine.
4. **`answer_dialog` returns once the tab has heard the dialog close.** The browser's reply to the answer and its event that the dialog closed travel separately, and the tab's state follows the event. Returning on the reply alone let the next call run before the event was heard. *Instead of:* clearing the state when the reply arrives, which would also clear a second dialog that opened in between.

## Consequences

- `LaunchReason` has a fifth case, `NoAnswer`. The crates above do not match on it.
- A program that is not a browser and never closes its pipe holds the start gate for the whole ceiling. It must be declared in a manifest to be started at all.
- An observing tab records a refusal before it fails the request, for the same reason as point 4: what the page can already see, the engine can already report.

## Revisit if

- One cold start alone is measured on the hosted Windows machine. The ceiling then follows the figure.
- A real installation needs several browsers to start together, and the gate is what makes it slow.

## Not proven

The gate and the ceiling have not yet run on Windows. The test that kills a browser under a call, and the dialog path, have not yet run to their end there.
