# 0095. The settings loader and the in-process runner, as built

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** key names in [spec section 226](../spec/2-kernel/226-configuration.md), which were a draft until the loader was built ([record 0066](0066-configuration-in-layers-that-may-only-tighten-and-a-bar-that-is-measured.md)).

## Context

Section 226 gave the groups, defaults, scopes, and protected settings, and left key names open. The loader needs every key to be one typed value, and three presets to be plain data. The runner loop met three facts of the crates below it that the designs did not foresee.

## Decision

**The loader.**

1. **Every key is one value.** A setting that the section wrote as a table is written as its parts: `engine.shadow.matches_to_promote.read` and `.write`, `authoring.scout.budget.seconds`, `.requests`, and `.depth`, `authoring.capture.limits.seconds`, `.bytes`, and `.exchanges`, `server.tls.certificate` and `.key`. `engine.time_limit_ceiling` is `engine.time_limit_ceiling_seconds`. A key with a name that arrives with a pack in it, such as `ledger.intent_window.<mode>`, is declared once with a `*` part.
2. **Six keys are added so that the presets are data.** `policy.agent_effect_ceiling`, `policy.pre_approval`, and `policy.writes_outside_tests`, all protected; `authoring.capture.enabled`; the value `off` of `authoring.scout.run`; and `providers.disabled`, the first-party packs switched off, which is what the provider registry is built from.
3. **Loosening is judged against the value in force below the layer.** Each protected setting declares its safer direction: lower, higher, off, a shorter list, empty text, or a rank of words. A preset and layers 5 to 8 are refused a value on the looser side of what the layers below them made. The installation file may loosen.
4. **Scope decides which layer may set a key.** An installation-only key is taken from the installation file and the flags of `serve`; a user-scope key not from the project file. Either is `policy`/`setting_protected`. A locked key changed by a higher layer is `policy`/`setting_locked`. An unknown key, a wrong type, and a file that is not TOML are `input`/`settings_invalid` with the file and the line.
5. **A pack's group is checked by its manifest's own code**, `Manifest::validate_settings`, one key at a time, so the loader knows no pack's types. A protected pack setting names its safer direction in a word; a word the loader does not know makes every change a loosening.
6. **Variables are `A2M_<GROUP>__<KEY>`**, lower-cased with `__` as the dot. A variable without `__`, such as `A2M_HOME`, is not a setting.
7. **`Settings::load` stops at the first problem and counts the rest; `Settings::load_all` returns every problem** and the settings the remaining values make, for `doctor.run` and `config.show`.

**The runner loop.**

8. **In one process the lease is at least 100 seconds.** The engine sends a committing step only when the lease leaves time for the whole binding, and the default step limit is 30 seconds, so a 30-second lease would hand a write out for ever. `runners.lease_seconds` is a floor there. A job the engine gives back with most of a full lease left, and no directive to pause, stops its run with `busy`/`lease_too_short` instead of being handed out again.
9. **A job runs on a thread of its own, driven by the same runtime.** The engine's future is not `Send`, because a driver's prepared state is held across its waits.
10. **`verified` is recorded through the ledger's `settle`.** The store runs a transaction on its own thread and takes only what it can own; the proof of `acted` is borrowed and cannot be copied. `settle` with `Landed` makes the same comparison under the same lease. The event is written as `reconcile_read`.
11. **A session's probe is not run.** The keeper is told that nothing is known, and relies on the signs of expiry a driver reports.

## What follows

- Section 226's tables carry the keys of items 1 and 2.
- Items 9 and 10 are workarounds for the crates below, reported to the lead: a `Sync` prepared state, and a way to mark a record verified from an owned value, would remove them.
- The evidence-space row of the health table is not built: no portable way to read free space is in the tree.

## Rejected

- **Table-valued settings.** Each would need its own type, its own rule for loosening, and its own form as a variable.
- **Lowering the engine's default step limit to fit a 30-second lease.** It changes how a real system is treated, to suit a number that only matters across machines.
- **Judging loosening against the built-in default.** A user file could then undo what the installation file tightened.
