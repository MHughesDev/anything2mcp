# 0083. The policy engine's interface, as building it settled it

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

## Context

`a2m-policy` was built against the store as [decision 0080](0080-the-store-is-one-sqlite-file-and-a-crate-reaches-its-own-tables-through-the-transaction.md) settled it, and several points of the [crate's design](../architecture/crates/a2m-policy.md) changed or were settled. They are recorded together.

## Decision

- **Calls that touch a table are plain functions over `&mut Tx`.** The store runs a transaction inside a closure on its own thread, so `decide`, `consume`, `publish`, `explain`, `check_limits`, and `set_kill_switch` are not `async`. `decide_alone`, `approve`, and `decline` run a transaction of their own. *Instead of:* `async` functions holding a transaction across an await.
- **The approval handle comes out of `Policy::open`, once.** `open` returns the engine and an `ApprovalSurface`. Recording an approval or a decline is a method of that handle and of nothing else; it is not `Clone` and has no public constructor. *Instead of:* methods on `Policy` that the action registry was trusted not to expose.
- **The version in force is read from the store at every decision.** `decide` asks for the newest version and loads it when it differs from the one in memory. So `health` and `kill_epoch` take a transaction too. *Instead of:* a set swapped in memory after a commit, which is wrong for the moment between commit and swap, wrong after a rollback, and wrong for a second process on the same store.
- **A request names its run and its diff.** `Request` carries `run` and `diff_hash`. `WriteWithoutPerson` is allowed only when both are present, the decision row carries both, and the ledger checks them when writes begin. *Instead of:* a pre-approval that any run could present.
- **The context holds no time and no client label.** Time is the store's clock, read once for each transaction. The client's name for itself is on the run row, redacted by the store; policy never sees it, so nothing can be decided on which make of agent is calling. `session_owner` is gone too: the identity a run would act as is the resource of `ActAs`, and a person's identity names its owner.
- **`PerformAction` carries the role the action needs.** The action registry states the role for each action; the request carries it; the policy set says who holds which role. *Instead of:* a table of action names inside the policy crate, which cannot know the registry.
- **Three outcomes from two answers.** `Decision::outcome` gives allow, needs approval, or refused with a failure. A denial of `WriteWithoutPerson` with no refusal is "needs approval"; under a kill switch the same question is refused.
- **Kill switches stop work, not administration.** They cover `UseTool`, `ActAs`, `WriteWithoutPerson`, and `SetOption`, at four scopes: everything, a workspace, a target, an operation. They do not cover `PerformAction`, `Approve`, or `SeeLabel`, so a switch can always be released.
- **Loosening needs a person present; tightening does not.** The crate's design said publishing a tighter local set needs the user-presence check; spec section 226 says tightening a protected setting is never protected. Section 226 stands. A set looser than the one in force on the record limit, the approval window, who approves, or the pre-approval rules is published only with a presence check that proves presence, on one machine and on a server.
- **`policy.approve_writes` has two values here.** `person_each_run` and `named_approver`. `caller_confirms` has no meaning yet, since a person approves every write through the surface.
- **A decline is its own row.** `approval_declines` holds who declined, or nobody, and a decline revokes the run's unused approvals. *Instead of:* a row in `approvals`, whose approver column cannot be empty.
- **One layer.** `PolicyLayers` is one layer of roles, grants, rules, quotas, settings, and tests. The lower layers that only narrow are not built.

## Consequences

- `a2m-actions` opens the policy engine and must move the `ApprovalSurface` to the approval surface. It reaches no approving call any other way.
- Every caller of `decide` must let its transaction commit on a denial, by returning `Ok` from the closure, or the record of the refusal is lost.
- The speed target of a decision now sits beside two small reads of the store on each call: the newest version's digest and the kill switches that are on.

## Alternatives considered

Given with each point above.

## Revisit if

A second database is put behind the store's transaction, or the reads at every decision show up in a measurement.

## Spec sections

- [212](../spec/2-kernel/212-policy-and-approvals.md), [226](../spec/2-kernel/226-configuration.md)
- [The crate's design](../architecture/crates/a2m-policy.md)
