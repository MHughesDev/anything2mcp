# 0427. A mode states how long its prepare may take, and the engine honours it under a ceiling

**Status:** Accepted. Design decision, made by a worker under the lead's brief. The owner had not read this record when it was written. It adds to two public contracts, all additions: the provider manifest's `[[modes]]` table takes `prepare_within_ms`, and `a2m-provider` gives `ModeDecl::prepare_within`, `DriverHandle::prepare_within`, and the constant `PREPARE_CEILING`. No failure code is added or changed.

**Date:** 2026-11-24

**Changes:** `a2m-provider` (`src/manifest.rs`, `src/driver.rs`, `src/registry.rs`); `a2m-engine` (`src/run.rs`, `src/engine.rs`); the web pack's manifest (`a2m-pack-web-ui`); [section 201](../spec/2-kernel/201-archetypes-and-modes.md)'s table of what a mode declares; the designs of `a2m-provider`, `a2m-engine`, and `a2m-pack-web-browser`; the technical reference. Builds on [decision 0134](0134-one-browser-starts-at-a-time-and-a-silent-start-is-slow-not-foreign.md).

## Context

The engine runs a driver's `prepare` under one number, `EngineSettings::prepare_timeout`, 30 seconds, whatever the mode. The web pack's `prepare` starts the browser when none is open, and decision 0134 gives a browser 120 seconds to start, because a first start on a cold hosted Windows machine was measured past 30 seconds. So the engine dropped a slow first start at 30 seconds, long before the pack's own deadline would have judged it, and the run failed `transient`, `prepare_timed_out`, on exactly the machine the 120 seconds were set for. Since the `Unrecorded` guard in the browser engine the drop leaves nothing behind, but the start is still lost, and the next call starts the cold browser again.

The kernel must not know about browsers: which kind of system is slow to prepare, and why, is a pack's knowledge (rule 8).

## Decision

1. **A mode's manifest may state the longest one `prepare` of its driver takes,** `prepare_within_ms`, in milliseconds. The engine gives `prepare` that long. A mode that states none is given `prepare_timeout`, 30 seconds, as before. *Instead of:* (b) raising the engine's single number, which gives every mode the slowest pack's time, so that an HTTP client that hangs holds a job for minutes, and which still breaks the next time any pack needs more; or (c) the web pack shortening its launch deadline under 30 seconds, which undoes decision 0134 on the very machine that showed 30 seconds is too short.
2. **The kernel holds a ceiling, `PREPARE_CEILING`, ten minutes.** A manifest that states zero or more than the ceiling is refused at registration, `input`, `manifest_invalid`; it is not cut down to fit (rule 7). A prepare that would need longer is a machine that is not working.
3. **A prepare past its time fails as today,** `transient`, `prepare_timed_out`, and its future is dropped; the driver contract already requires that to be safe.
4. **The web pack states three minutes for each of its modes:** the launch deadline of two minutes, then the protocol calls that open the tab and place its state. A test of the pack holds the figure at or above `launch_deadline` and `call_deadline` together, as `BrowserSettings` gives them by default.

## Consequences

- A slow first browser start within its launch deadline completes; the engine no longer decides how slow a browser may be.
- The time is the provider's statement, as `absence_steps` is: a plug-in can state a long one, and the ceiling bounds what that costs.
- A person who raises `launch_deadline` in the pack's settings past what the manifest states is again dropped by the engine first. The manifest is fixed data; the setting is not. That gap is written down, not closed.
- The engine's `prepare_timeout` keeps its name and meaning for every mode that states nothing, which is every mode outside the web pack today.

## Revisit if

- One cold browser start alone is measured on the hosted Windows machine (decision 0134's own condition): the launch deadline, and the three minutes with it, then follow the figure.
- A pack needs its prepare time to follow a setting. A driver could then state its time when the pack is registered, from its settings, in place of the manifest's fixed figure.

## Not proven

The longer time has not met a real cold machine. The engine tests prove it with a test driver whose `prepare` waits in the test's time: ninety seconds within a stated two minutes completes, a hundred and fifty seconds fails `transient`, `prepare_timed_out`, and forty-five seconds on a mode that states nothing fails at the default.
