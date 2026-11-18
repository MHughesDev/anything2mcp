# 0288. Whether a page shows a sign now is the page's own answer, and a slow answer is never a no

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** the `Look` interface of `a2m-pack-web-browser`, which gains `holds`; and how `a2m-pack-web-ui` asks whether a sign holds now, in its sign-in, its driver and its page requests.

## Context

The web interface pack asks many times whether a page shows a sign now: is this the sign-in screen, was the sign-in refused, is an expiry screen up. It asked by waiting for the sign for a fixed 250 milliseconds and reading a wait that ran out as "no".

On the hosted macOS runner, for pull request 16, a sign-in over the test application stopped with `unexpected_page`: the page showed none of the signs the target declares. The page did show one. The browser had not answered within 250 milliseconds, and the pack took that for a page without the sign. Rule 7 says unknown means stop, and the pack did stop, but it named the stop wrongly: it said the page was a variant it did not know, when it did not yet know what the page was.

The fixed wait also cost time. Each sign that did not hold took 250 milliseconds from the one deadline a sign-in has. Measured on Linux, 2.25 seconds of a 5 second sign-in went to nine such waits.

## Decision

1. **`Look::holds(condition, deadline)` asks the page once.** The page looks at once and answers yes or no. The deadline bounds only the browser's reply.
2. **No reply by the deadline is `timeout`, never "no".** A caller that asked whether a sign holds learns yes, no, or that the page did not answer; it never reads the third as the second.
3. **The web interface pack uses `holds` wherever it asked "now".** The 250 millisecond look is gone from the sign-in, the driver and the page requests. Each asks within the step's own deadline.
4. **A wait stays a wait.** `Look::wait` is unchanged for a caller that means to wait for a sign to appear.

## Alternatives

- **A longer fixed look.** It moves the fault to a slower machine and costs more time on every sign that does not hold.
- **A longer deadline in the test's target.** The fault was the product's: any target on a slow machine could be told its page was unknown.

## Consequences

- A sign-in over the test application takes about half the time it took.
- A page that hangs still stops the step at the step's deadline, as `timeout`.
- Not changed, and at risk only if one round trip to the browser takes over 500 milliseconds: the driver's 500 millisecond slices of a wait that re-arms until the step's deadline.
- Proof: in `a2m-pack-web-browser`, a page kept busy gives `timeout` to `holds` and not "no"; the web interface pack's tests pass pinned to one processor. The macOS run that showed the fault is the proof still owed.
