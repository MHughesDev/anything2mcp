# 0999. A person-driven discoverer is held to its bound by the handle, and has a code for the watch it lacks

**Status:** Accepted. Design decision, made by the agent that built package E2a. The owner had not read this record when it was merged. It settles three points [decision 0395](0395-a-person-driven-discoverer-is-called-through-a-method-of-its-own-and-the-scope-is-left-as-it-is.md) left open, and changes nothing it decided.

**Date:** 2026-11-24

**Changes:** how `DiscovererHandle::watch`, `CaptureEnd`, and the default of `Discoverer::watch` are built in `a2m-provider`.

## Context

Decision 0395 says a person-driven discoverer returns when the capture's end is set or a bound is hit, that the end is set by the kernel and not made by a pack, and that the default `watch` fails with "the contract's own code for a step that is not usable". Three things were not said: who holds the bound, how the kernel gets an end it can set while a pack cannot, and which code the default gives. No existing code says that a discoverer lacks the method its manifest promises: `step_kind_unknown` is about a mode's steps, and `not_registered` about a registration that is missing.

## Decision

1. **The handle holds the bound, not the discoverer.** `DiscovererHandle::watch` takes the earlier of the scope's deadline and the scope's `seconds`, held to the manifest's `seconds`. When it passes before the end is set, the handle sets the end itself, and the report's `bounds_hit` names `seconds`. A discoverer that keeps no bound is still ended at one, as the capture job of package E2b needs.
2. **Once the end is set, the discoverer has a grace of five seconds to return** (`CAPTURE_END_GRACE`). It has only to hand over what it holds. One that does not is dropped and the watch fails `transient`/`timeout`, so a pack's fault cannot keep a capture job alive past its bound. The grace is a deadline on a condition, not a fixed wait.
3. **The end has two halves.** `CaptureEnd`, which a discoverer is handed, can be asked and waited on, and has no public way to set it or make it. `CaptureStop`, the kernel's half, sets it, and comes only from `Registry::capture_stop`, as a `DriverHandle` comes only from the registry. No pack is given a registry.
4. **The default `watch` fails with a code of its own,** `unknown_variant`/`discoverer_cannot_watch`: a discoverer declared as driven by a person was built without `watch`. It is the provider author's fault, as `finding_not_declared` is, so it takes that class.
5. **`Registry::files_taken` counts only discoverers the engine drives,** since only they run in a discovery, which is what is handed the files.

## Alternatives

| Alternative | Why not |
| --- | --- |
| The discoverer keeps its own bound | Then a discoverer that forgets it holds a capture open for ever; the bound is the kernel's (decision 0392, point 2: "its bounds") |
| One `CaptureEnd` with a public `set` | A discoverer could then end its own capture and the job could not tell that from the person's stop; harmless but untrue to "the kernel sets it" |
| Reuse `step_kind_unknown` or `not_registered` for the default | Each already means something else, and a person reading the failure would look in the wrong place |
| No grace: drop the discoverer the moment the end is set | It would lose the findings of the last moment, which a discoverer flushes on its way out |

## Proof

`crates/a2m-provider/tests/watch.rs`: `a_watch_returns_at_its_end_and_at_its_bound_when_the_end_is_never_set`, `a_discoverer_that_does_not_return_after_its_end_is_given_up_as_a_timeout`, `a_discoverer_a_person_drives_that_does_not_watch_fails_by_the_contracts_default`, and `the_registry_gives_a_person_driven_discoverer_to_a_capture_and_never_to_a_discovery`.

## Revisit if

A capture job needs to tell a person's stop from a closed window or a bound by the end alone, or a real person-driven discoverer needs longer than the grace to hand over what it holds.
