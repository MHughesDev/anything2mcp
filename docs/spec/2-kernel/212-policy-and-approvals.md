# 212. Policy and approvals

A policy gate sits between the front doors and the engine. Every call passes through it, and it can only narrow what a run may do. Behind the gate is the policy engine, `a2m-policy`: the one place that decides whether something may happen. Front doors, the scheduler, the engine, and runners all ask it; none of them decides for itself, and no other crate holds a grant. This section says which path each kind of call takes, the controls a call meets in order, how the policy engine decides and records its decisions, how a person approves a write, and the limits and switches beside it. What may be damaged beyond a record, and when a person must consent before discovery, are [section 213](213-effects-hazards-and-consent.md). What a person sees when asked to approve is [section 503](../5-security/503-approval-and-sign-in-surfaces.md). Who may call what, toolsets, and tenancy are [section 214](214-access-control-toolsets-and-tenancy.md). The crate is designed in [`a2m-policy`](../../architecture/crates/a2m-policy.md). Built today: partly; [section 704](../7-project/704-readiness.md) gives the detail.

## Effect classes

The effect class of an operation decides the path a call takes. It is derived by the compiler from the target's files, and a pipeline's class is the worst of its operations'.

| Effect | Examples | Preview | Approval | Automatic retry |
| --- | --- | --- | --- | --- |
| `read` | Get a record, list records | No | None | Yes |
| `write` | Create or update records | Required | Per policy: the caller's own confirmation, or a named approver | Idempotent steps only |
| `irreversible` | Delete, submit, send, pay; any write that cannot be read back | Required | A person, every run; optionally two people | Never |

An operation's hazard can raise the approval its effect class asks for, and never lower it ([section 213](213-effects-hazards-and-consent.md)). Any hazard other than `none` means a person approves every run and policy cannot pre-approve it; `irreversible` operations and the hazards `device_function` and `physical` can never run under a standing approval.

## The controls, in the order a call meets them

1. **Authentication and grants.** The caller is identified and must hold a grant for the operation and its effect class.
2. **Permission manifest.** The run may touch only the surfaces, downloads, uploads, and destinations the operation declared.
3. **Input validation.** Inputs and the whole source are checked first. Bad input returns every problem at once and changes nothing.
4. **Preview.** The read steps run and produce a diff of current against proposed values, record by record ([section 208](208-runs-the-ledger-and-resume.md)).
5. **Approval.** A person approves that exact diff, unless policy pre-approved it (below).
6. **Limits.** Maximum records per run, rate budgets per surface, quotas per caller, and kill switches.
7. **Read-back.** Every write is read back and compared, or is declared `irreversible` and approved as that.

The policy engine's order of evaluation makes this exact. For a call that writes:

1. Kill switches.
2. Authenticate the caller and load what is known about them.
3. May this caller use this tool?
4. Resolve the identity from the toolset; may the run act as it?
5. Check the operation's permission manifest.
6. Validate inputs and the source; apply record limits and quotas.
7. Preview.
8. May this write proceed without a person's approval? If not, wait for one, and check that the approver may approve.
9. Just before the write phase, and again on any resume: repeat steps 1 and 3. Grants can change while a run waits, and a person who loses a grant while their run waits does not get the write. A run is resumed only by the person who started it or a caller proved to act for them; the question is asked about whoever resumes, and again, at guard, about the caller that started the run.
10. When results return: mask fields the caller may not see.

Reads stop after step 6 and go straight to step 10.

## The policy engine

### Properties

1. **Deny unless permitted.** Nothing is allowed by default.
2. **A forbid always wins.** No permit, at any level, overrides a forbid.
3. **Policy only narrows.** A target file, a toolset, or a caller can tighten what is allowed, never widen it.
4. **Deterministic.** The same request against the same policy version gives the same answer.
5. **Fail closed.** Any error while deciding is a denial. With no valid policy version loaded, every decision is a denial and the kernel serves nothing but its status ([section 224](224-when-the-engine-itself-fails.md)).
6. **Explained.** Every decision records which rules decided it and under which policy version.

### The decision core

Authorization decisions are evaluated by `a2m-policy`'s own evaluator ([decision 0082](../../decisions/0082-the-decision-core-is-the-policy-crates-own-evaluator.md), which supersedes the choice of Cedar in decision 0033). Its semantics are the first two properties above: a request is denied unless a permit applies, and a single matching forbid denies it. The forbids are the built-in guardrails, written as functions over the typed request and consulted before any permit. The permits are data: roles, grants, label and option rules, and pre-approval rules. A property test over generated requests holds the two semantics.

**An error is a denial.** No rule can be skipped. The only thing that can fail while deciding is the condition of a pre-approval rule, an expression of `a2m-expr`. It is checked when the set is compiled, and one that still fails when it runs turns the decision into a denial with the code `policy_evaluation_failed`.

Brief 31 measured Cedar before the decision core was built: its tree carries a licence expression outside the closed list, 107 crates, and 5.5 MB of binary. Decision 0082 has the numbers. Nothing outside `a2m-policy` sees the evaluator, so a policy language can still be put behind the same interface.

### The questions

The policy engine answers a fixed set of questions. Each is one authorization request about a principal, an action, and a resource, with context, and each answer is a plain allow or deny.

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

### What a request is about

| Kind | Members | Arranged as |
| --- | --- | --- |
| Principals | People, groups, AI agents with their own identity, programs with a key | People belong to groups; an agent records the person it acts for, if any |
| Resources | Operations and pipelines | Tool in module in target in workspace; toolsets and environments alongside |
| Other resources | Sensitivity labels, runs, target versions, runners, sessions | Flat |
| Context | Effect class, hazard, record count, toolset, environment, client, time, identity mode, the session owner, whether the caller is an agent and for whom it acts | Built by `a2m-actions` from verified facts and from what the engine reports, never by the caller |

Because tools sit inside modules inside targets, one grant on a target covers every tool in it, and a grant on a module covers that module.

The engine never sees a policy type. At guard it asks through `Control`, with its own question naming the run, the tool, the effect class, the hazard, and the binding chosen; `a2m-actions` turns that into a policy request ([section 207](207-the-engine-loop.md)).

### Who writes the rules

| Layer | Written by | Can it be overridden? |
| --- | --- | --- |
| Built-in guardrails | Anything2MCP | No. They are forbids |
| Organization | Administrators | No lower layer can permit what it forbids |
| Workspace | Workspace administrators | The same, within the workspace |
| Toolset | Whoever defines the toolset | Narrows only |
| Target files | The target's author | Limits and declarations; narrows only |

A lower layer is compiled as a further restriction. A target file, toolset, or workspace layer that holds a permit wider than its parent's does not compile.

**Built-in guardrails**, which no configuration can remove. They are compiled into the binary with their own hash and added to every policy set, so no published version lacks them.

- An agent's identity can never approve a run or a publish.
- Where policy names an approver, the caller's own confirmation does not count as approval.
- The author of a version cannot be its only publisher.
- An `irreversible` operation never runs without a person's approval, and never by standing approval.
- A run can touch only the surfaces its operation declared.
- A caller can never name the identity it runs as.
- A session signed in as a person is used only by that person or an agent acting for them, as [section 215](215-sessions-and-the-session-keeper.md) defines. No role, including administrator, can run as someone else. A delegation from a person to an agent counts only while it is unrevoked and unexpired, and only for the toolset it names.
- The hazard rules of section 213: any hazard other than `none` forbids writing without a person; `device_function` and `physical` forbid a standing approval.

**Grants, not a language, for everyday use.** Administrators write grants in the console or a file with `grant.add`: a principal, a scope (target, module, or name pattern), a highest effect class, and an identity mode. Each grant is one permit. Roles are bundles of grants. A pre-approval rule may carry a condition in the expression language; nothing else is written in a language.

### Policy is a versioned, tested artifact

[Decision 0034](../../decisions/0034-policy-is-a-versioned-tested-artifact-and-every-decision-is-recorded.md) makes the policy set something published, like a target.

- The policy set is versioned, immutable, and hashed. Changing it means publishing a new version with `policy.publish`.
- **Policy tests.** A policy set ships with a table of requests and the decisions expected for them. `policy.test` runs it, and it must pass before a version can be published, so a change that would let a viewer write, or lock out an approver, is caught first.
- A new version takes effect atomically. Every decision records the version that made it, and runs that are waiting are judged again under the new version at their next check.
- Publishing policy is an admin action, audited, and can require a second administrator.

### Every decision is recorded and can be explained

- Every decision is stored, whether it allows or refuses: who asked, what for, the answer, the rules that determined it, and the policy version. A test over the whole action registry proves that every dispatch records one.
- `policy.explain` shows why a past call was allowed or denied. Only an auditor or administrator may run it.
- "What could this person do?" and "who can call this tool?" are answered by running the questions over the catalog, without waiting for a real call.
- A denial tells the caller the code and what to ask for, never the content of the rules.

| Code, class `policy` | When |
| --- | --- |
| `not_granted` | No permit applies to the caller, the tool, or the action |
| `identity_not_allowed` | The run may not act as the identity, including every attempt to use another person's session |
| `effect_too_high` | A grant exists, with a lower highest effect class |
| `approval_missing` | A write reaches the write phase with no usable approval |
| `approval_stale` | The approval is expired, used, revoked, or for another diff |
| `approval_refused` | The approver is not a person, may not approve this run, or the run is not waiting for approval. Also the failure of a run that ended `expired` because a person declined it or its approval window ended |
| `limit_exceeded` | Records per run or a quota |
| `kill_switch_on` | A kill switch covers the request |
| `policy_evaluation_failed` | An error while deciding, or no valid set is loaded |
| `presence_required` | A set that loosens a protected setting, or the release of a kill switch on one machine, with no user-presence check |
| `second_admin_required` | A set that needs a second administrator to publish it was offered without one, or the second one may not review it ([decision 0328](../../decisions/0328-a-toolset-only-narrows-a-role-is-a-bundle-of-grants-and-a-second-administrator-is-a-named-check.md)) |

A candidate set that fails validation, or a lower layer that widens a higher one, is refused with `input`, `schema_invalid`; one that fails its own test table, with `input`, `policy_tests_failed`.

### Where facts about people come from

- On a server, identity and group membership come from the caller's verified token and from a directory sync ([section 602](../6-running-it/602-identity-on-a-shared-server.md)).
- A user disabled in the identity provider stops being authenticated when their token no longer validates, and the sync revokes their stored sessions.
- Group changes take effect within the token's lifetime or the sync interval, whichever is shorter. That bound is shown in the admin console.

### On one machine

The local engine uses the same policy engine with a built-in policy set: the operating-system user holds every role, writes need approval, and the guardrails apply, including that an agent cannot approve. A person can tighten this set but not loosen the guardrails. The approval rules and record limits are protected settings, so publishing a looser local set needs the operating system's user-presence check; a tighter one needs none ([section 226](226-configuration.md), [decision 0083](../../decisions/0083-the-policy-engines-interface-as-building-it-settled-it.md)).

## Approvals

An approval is a person's decision to allow one exact diff. It is stored server-side as a **single-use record bound to the diff's hash**, together with the tool version, the run, the approver, the channel it arrived by, the user-presence check passed, and an expiry. The diff and its hash are defined in [steps, snapshots, and diffs](../../reference/steps-snapshots-and-diffs.md#diffs).

- **Recording.** An approval is refused unless the proof of presence handed in with it is the approver's own and was made within two minutes, the approver is a person and not an agent identity, policy allows them to approve this run, the diff hash and the tool version equal the run's, and the run is waiting at `needs_approval`. Where policy names an approver, the caller's own confirmation is refused. Where `policy.two_person_irreversible` is on, an irreversible run needs the approvals of two people.
- **Using.** When the write phase begins, the approval is marked used in the same transaction in which the ledger moves the run to `running`. An expired, used, or revoked approval, or one for another diff, is refused, and so is any approval of a run that was declined or that a kill switch covers when it is used (rule 3 of [section 208](208-runs-the-ledger-and-resume.md), [decision 0145](../../decisions/0145-only-the-policy-engine-clears-a-write-and-the-ledger-reads-the-whole-clearance.md)). A resumed run continues under the approval it already used.
- **Freshness.** If the system's data changes before the write, the run stops at that record as `stale`. The check is made per record, just before each write, when the preview is older than the freshness window.
- **Silence is a decline.** An approval window that ends is recorded as a decline by nobody, and the run ends `expired`.

**Recording an approval is not an action.** Every other way into the kernel is an action with a generated command, tool, route, and SDK method ([section 220](220-actions-and-the-authoring-interface.md)). Approving or declining has none of them, so that no command, tool, or route can approve. The approval surface is handed a capability that can record an approval or a refusal and nothing else: on one machine, the engine's own prompt after the operating system's user-presence check; on a server, the console's page after the approver signs in. A test proves that the registry holds no action able to approve, on any face.

**Approval channels.** An agent must never be able to approve its own action, so approvals arrive only through channels the calling agent cannot operate.

| Channel | How it works | Fits |
| --- | --- | --- |
| MCP URL-mode elicitation | The server sends the client a link to Anything2MCP's approval page; the person approves in their browser, signed in as themselves. The MCP specification reserves URL mode for sensitive interactions that must not pass through the client ([MCP specification](https://modelcontextprotocol.io/specification/2026-07-28/client/elicitation)) | Clients that support elicitation |
| Approval page link in the result | The result carries `needs_approval` and a link; the person opens it | Every client, including ones without elicitation |
| Chat approval | An approval card sent to the approver in the organization's chat system | Company deployments |
| Local prompt | A prompt raised by the engine itself, with an operating-system user-presence check where one exists; there is no approve command or tool ([section 222](222-front-doors-and-the-local-engine.md)) | Solo use |

**Avoiding approval fatigue.**

- One approval covers a whole pipeline diff, so a 40-record sheet is one decision.
- Policy can pre-approve low-risk writes, such as a user updating their own records under a size limit, while keeping the preview and audit. The run then enters the write phase with the recorded decision instead of an approval.
- Standing approvals can be granted for a named pipeline, caller, and time window, and are revocable. They never cover `irreversible` operations or the hazards `device_function` and `physical`.

## Limits, quotas, and kill switches

These are counts and switches, not authorization rules, and are handled by a limiter beside the decision core.

| Control | Scope | Enforced |
| --- | --- | --- |
| Maximum records per run | Pipeline, toolset, workspace | At validation, before any preview |
| Request budget | Surface | By the control plane, granted to runners in batches |
| Quota | Caller, per period | When a call is accepted |
| Parallelism | Session, target, system | By the scheduler ([section 211](211-concurrency.md)) |
| Kill switch | Operation, target, workspace, whole system | Checked first on every decision about work on a system; reaches runners with the next heartbeat |

A kill switch is set with `killswitch.set`. It does not stop administration or approving, so it can always be released. It stops new work at once: the scheduler stops handing out jobs it covers and tells runners. Runs in flight stop at their next commit point and stay resumable.

## Content from the system, and published versions

Text read from a system, such as notes and comments, enters the calling agent's context and may contain instructions aimed at the agent. Results are structured; free text sits only in fields marked untrusted, limited in length and stripped of markup and hidden characters; the permission manifest blocks undeclared destinations, so a steered agent cannot use a tool to send data elsewhere; and writes still need a person's approval of a visible diff. [Section 502](../5-security/502-defending-against-a-manipulated-agent.md) gives the full defense.

Published versions are immutable, hashed, and signed by the publisher, and a server loads only versions whose signature and hash are in its registry ([section 206](206-the-compiler-and-the-compiled-target.md)).

## Speed targets

A decision on a warm cache: under 100 microseconds. Loading a new policy version: under 100 ms. Both are targets until measured, from slice 3.2 (hypothesis H-11).

## Open points

- **Whether decisions on reads may be sampled.** Decision 0034 allows reads' decisions to be sampled or summarized to save space; the properties above say every decision is recorded. Until this is settled, every decision is stored in full.
- Whether asking tool by tool is fast enough to list the tools a caller may use over a large catalog. One decision of the core takes about 0.07 microseconds; the two reads of the store beside it are not measured. Hypothesis H-11.
- Whether administrators ever need a policy language. Today they write roles, grants, and pre-approval rules, whose conditions are expressions (decision 0082).
- The exact claims to rely on for groups and roles from each identity provider.
- The shape of the presence proof the local approval surface hands in. Settled in part by decision 0145: each operating system's prompt has its own word (`polkit`, `local_authentication`, `windows_hello`, and `field_harness`, which only a copy test's harness records, decision 0310), and the proof's time and prompt name come with the approval.
- Consent records are kept by `a2m-policy`, because only the decider should ask whether one exists; whether they belong with the scout instead is open.
- The approval and steering defenses rest on design judgment and the MCP specification; dedicated research on approval patterns in organizations is still to do.
