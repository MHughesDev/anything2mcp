# 0999. A rollback is a person's act, and moves to any version a person published

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-25

**Changes:** `version.rollback` in `a2m-actions`, built under [decision 0465](0465-while-the-first-release-waits-on-the-owner-agents-build-the-one-machine-packages-moved-after-it.md); the `rollback` shortcut, which is now offered; the [action catalog](../reference/action-catalog.md).

## Context

Spec [section 224](../spec/2-kernel/224-when-the-engine-itself-fails.md) says one action moves a target's pin back, and runs in flight finish on the version they started with. Section 604 says a person rolls a target back in one step. Neither says who may do it, nor whether the version must be older than the current one. The catalog defined the action for the publisher's role, with no person's act. The store already keeps every published version and marks one as current, and a run keeps the hash of the compiled target it started with.

## Decision

1. **A rollback is a person's act** (`PersonNeed::Presence`), as a publish is. An agent's call is handed to the person surface's page, or refused with `person_required` where there is no page. A call at a terminal raises the presence prompt. A rollback decides which tools run, and which writes they can make. The later version may have removed a write on purpose, so an agent that rolls it back would bring that write back with no person seeing it.
2. **It moves to any version a person published, earlier or later.** The version named must be stored for the target. A rollback to an earlier version can be undone by the same action, back to the later one. Without that, the later version could come back only through a new publish under a new number. `version.activate`, with its candidate stage, remains the action for a new version that has not run yet.
3. **Naming the version that already runs changes nothing.** The answer says `changed: false` with a warning, so a call made again after a lost answer is safe.
4. **The move is recorded and then proven.** One transaction moves the current mark and writes the audit entry `version.rollback`, with the version, its hash, and the version it replaced. The store is then read again. The answer says the version is current only if the store holds it as current. Otherwise the answer is `stale`, `version_moved`, because a publish or another rollback came between. Each watch tied to another version pauses or ends, as after a publish.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Any caller with the publisher's role, agents included | An agent could bring back a write a person removed. Publishing is a person's act for the same reason |
| Only an earlier version | Undoing a rollback would need a new publish of the same drafts under a new number |
| Refuse the version that already runs | A retry after a lost answer would fail when it should succeed |

## Revisit if

A server needs a rollback that an operator does under a role, with no person's presence; or `version.activate` is built and its candidate stage should also hold a rollback.
