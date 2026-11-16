# 45. The policy engine

The policy engine is the one place that decides whether something may happen. Front doors, the scheduler, the engine, and runners all ask it; none of them decides for itself. This section turns [section 18](18-safety-policy-and-approvals.md) and [section 20](20-access-control-toolsets-and-tenancy.md) into a design: what is asked, how it is answered, who writes the rules, and how a decision is explained afterwards.

## Properties

1. **Deny unless permitted.** Nothing is allowed by default.
2. **A forbid always wins.** No permit, at any level, overrides a forbid.
3. **Policy only narrows.** A target file, a toolset, or a caller can tighten what is allowed, never widen it.
4. **Deterministic.** The same request against the same policy version gives the same answer.
5. **Fail closed.** Any error while deciding is a denial.
6. **Explained.** Every decision records which rules decided it and under which policy version.

## The decision core

Authorization decisions are evaluated with Cedar, an open-source policy language whose engine is a Rust crate. Its semantics are the first two properties above: a request is denied unless a permit applies, and a single matching forbid denies it. Its documentation describes forbid policies as guardrails that permit policies cannot cross ([research note 14](../research/14-sessions-and-policy-engines.md)). Using it means the hardest part, the meaning of overlapping rules, is specified and tested by someone else.

**One caution, handled.** Cedar skips a policy whose evaluation raises an error. A skipped forbid would fail open. Web2MCP therefore validates every policy against its schema before the policy set can be published, which rules out the type errors that cause skips, and treats any evaluation error reported with a decision as a denial.

Whether Cedar's size and speed fit the budgets below is to be confirmed by a spike before Phase 6. If it does not fit, the fallback is a small in-house matcher with the same two semantics, behind the same interface.

## The questions

The engine answers a fixed set of questions. Each is one authorization request about a principal, an action, and a resource, with context.

| Question | Asked when | A denial means |
| --- | --- | --- |
| May this caller use this tool? | On every call, and again before the write phase and on resume | `policy` / `not_granted` |
| May this run act as this identity? | When the identity mode is resolved from the toolset | `policy` / `identity_not_allowed` |
| May this write proceed without a person's approval? | After the preview | The run waits at `needs_approval` |
| May this person approve this run? | When an approval arrives | The approval is refused |
| May this caller see fields with this sensitivity label? | When a result is returned | Those fields are masked or left out |
| May this caller set this per-call option, such as pinning a mode? | On the call | The option is refused |
| May this principal perform this authoring or admin action? | On every action in the registry | `policy` / `not_granted` |

Treating "without approval" and "may see" as questions of their own keeps the answer to every question a plain allow or deny.

## What a request is about

| Kind | Members | Arranged as |
| --- | --- | --- |
| Principals | People, groups, AI agents with their own identity, applications with a key | People belong to groups; an agent records the person it acts for, if any |
| Resources | Operations and pipelines | Tool in module in target in workspace; toolsets and environments alongside |
| Other resources | Sensitivity labels, runs, target versions, runners, sessions | Flat |
| Context | Effect class, record count, toolset, environment, client, time, identity mode, whether the caller is an agent | Supplied by the engine, never by the caller |

Because tools sit inside modules inside targets, one grant on a target covers every tool in it, and a grant on a module covers that module.

## Who writes the rules

| Layer | Written by | Can it be overridden? |
| --- | --- | --- |
| Built-in guardrails | Web2MCP | No. They are forbids |
| Organization | Administrators | No lower layer can permit what it forbids |
| Workspace | Workspace administrators | The same, within the workspace |
| Toolset | Whoever defines the toolset | Narrows only |
| Target files | The target's author | Limits and declarations; narrows only |

**Built-in guardrails**, which no configuration can remove:

- An agent's identity can never approve a run or a publish.
- Where policy names an approver, the caller's own confirmation does not count as approval.
- The author of a version cannot be its only publisher.
- An `irreversible` operation never runs without a person's approval, and never by standing approval.
- A run can touch only the surfaces its operation declared.
- A caller can never name the identity it runs as.
- A session signed in as a person is used only by that person or an agent acting for them, as defined in [section 44](44-session-keeper.md). No role, including administrator, can run as someone else.

**Grants, not a language, for everyday use.** Administrators write grants in the console or a file: a principal, a scope (target, module, or name pattern), a highest effect class, and an identity mode. Each grant compiles to a permit policy. Roles are bundles of grants. Administrators who want more can write policies directly, and anyone can see the compiled form.

## Limits, quotas, and kill switches

These are counts and switches, not authorization rules, and are handled by a limiter beside the decision core.

| Control | Scope | Enforced |
| --- | --- | --- |
| Maximum records per run | Pipeline, toolset, workspace | At validation, before any preview |
| Request budget | Surface | By the control plane, granted to runners in batches |
| Quota | Caller, per period | When a call is accepted |
| Parallelism | Session, target, system | By the scheduler |
| Kill switch | Target, workspace, whole system | Checked first on every decision; reaches runners with the next heartbeat |

A kill switch stops new work at once. Runs in flight stop at their next commit point and stay resumable.

## The order of evaluation

For a call that writes:

1. Kill switches.
2. Authenticate the caller and load what is known about them.
3. May this caller use this tool?
4. Resolve the identity from the toolset; may the run act as it?
5. Check the operation's permission manifest.
6. Validate inputs and the source; apply record limits and quotas.
7. Preview.
8. May this write proceed without approval? If not, wait for one, and check that the approver may approve.
9. Just before the write phase, and again on any resume: repeat steps 1 and 3. Grants can change while a run waits.
10. When results return: mask fields the caller may not see.

Reads stop after step 6 and go straight to step 10.

## Policy is versioned like a target

- The policy set is a versioned, immutable, hashed artifact. Changing it means publishing a new version.
- **Policy tests.** A policy set ships with a table of requests and the decisions expected for them. The table runs before a version can be published, so a change that would let a viewer write, or lock out an approver, is caught first.
- A new version takes effect atomically. Every decision records the version that made it, and runs that are waiting are judged again under the new version at their next check.
- Publishing policy is an admin action, audited, and can require a second administrator.

## Explaining a decision

- Every decision is stored: who asked, what for, the answer, the rules that determined it, and the policy version.
- `web2mcp policy explain` shows why a past call was allowed or denied.
- "What could this person do?" and "who can call this tool?" are answered by running the questions over the catalog, without waiting for a real call.
- A denial tells the caller the code and what to ask for, never the content of the rules.

## Where facts about people come from

- On a server, identity and group membership come from the caller's verified token and from a directory sync.
- A user disabled in the identity provider stops being authenticated when their token no longer validates, and the sync revokes their stored sessions.
- Group changes take effect within the token's lifetime or the sync interval, whichever is shorter. That bound is shown in the admin console.

## On one machine

The local engine uses the same policy engine with a built-in policy set: the operating-system user holds every role, writes need approval, and the guardrails apply, including that an agent cannot approve. A person can tighten this set but not loosen the guardrails.

## Speed targets

A decision on a warm cache: under 100 microseconds. Loading a new policy version: under 100 ms. Both are targets until measured.

## Open points

- Cedar's binary size and decision time in practice, and whether its partial evaluation is stable enough to list the tools a caller may use without asking tool by tool.
- Whether to expose the policy language to administrators at all in the first release, or only grants.
- The exact claims to rely on for groups and roles from each identity provider.
