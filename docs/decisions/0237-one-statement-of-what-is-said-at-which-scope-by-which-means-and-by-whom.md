# 0237. One statement of what is said at which scope, by which means, and by whom

**Status:** Accepted. Design decision. Written by the configurability review of 2026-11-18; the owner has not read it. Points 3 and 4 change behaviour and are ready to build; the rest states the design as one rule.

**Date:** 2026-11-18

**Changes:** the scopes part of [section 226](../spec/2-kernel/226-configuration.md); the settings loader and the reading of the project file in `a2m-actions`; the engine's start in `anything2mcp`. Takes up findings 2 and 3 of the [configurability review](../reviews/configurability-review-2026-11-18.md).

## Context

A person who wants something to hold for one scope has three means: a setting, a target's files, and a policy set. Section 226 has a table of twelve scopes. The review found three things wrong with the picture it gives.

- A person cannot tell from it which means holds which choice. "Stricter in production" can be said by an environment's `approve_writes` in the target's files, by a policy condition on `environment`, or by the setting `policy.writes_outside_tests`, and on one machine only the first and the last can be written: nothing on one machine writes a policy set.
- The project scope is not what it says. The engine is one per person and serves every project; it applies the project file of the folder it was started in, and a request from another project has its file checked to parse and never applied. Nothing tells the person. When the variables differ, the person is warned (`settings_differ`); when a project file differs, they are not.
- The installation layer may loosen every protected setting, and section 226 says no agent that sets a variable or passes a flag can loosen one. `A2M_HOME` is a variable, and it moves the installation layer into a folder the person's own account writes, which is to say any program of theirs, an agent's shell included. On Windows the shared program-data folder lets any user create the folder the installation file is read from, by its default access list.

## Decision

**1. What each means is for.**

| Means | It says | Its scopes | It can | Written by, on one machine |
| --- | --- | --- | --- | --- |
| A setting | How this installation behaves: a ceiling, a floor, or a choice of how | The installation; the person; a project, for that project's requests | Narrow what a target declares; never widen where a request may go or loosen a guardrail | The installation layer: an administrator. The others: the person |
| A target's files | What a system is and how it must be treated, wherever the target runs | The target; a surface; an operation or pipeline; an environment | Declare limits, rates, effect, hazard, environments, test records; tighten the approval rule of one environment | The developer, then a person who publishes |
| A policy set | Who may do what, and when a write needs no person | A target, a module, or a name pattern; with conditions on environment, toolset, tool, records, effect, hazard, and whether the caller is an agent | Grant, refuse, pre-approve within the guardrails, count, and stop | Nobody yet on one machine; a person through `policy.publish`, with the presence prompt, once built |

Where two means can say the same thing, the stricter holds: an environment's `approve_writes` and the installation's; a published policy set's settings and the installation's (`Settings::tighter`); a pack's ceiling and a target's value.

**2. What no means says.** A setting never names a surface, an endpoint, a field, or a locator (rule 8). A target's files never loosen a setting. A policy set never loosens a policy setting or reaches a guardrail. None of the three tells one client from another by the make it claims (decision [0058](0058-a-toolkit-for-developers-with-no-catalog-and-no-preferred-agent.md)). A connection the person names, with its own toolset and effect ceiling, is not barred by that decision; it is not built, and the [breadth review](../reviews/breadth-review-2026-11-18.md) proposes it. A network and a time of day are not scopes yet: the first arrives with network awareness as a policy input and a reach per network ([decision 0222](0222-the-product-is-aware-of-the-network-its-machine-is-on.md)), the second as a policy condition on the engine's clock.

**3. A project's file applies to that project's requests.** The engine keeps the layers 1 to 5 and 7 to 8 it started with. For each request that names a checked project folder, it loads that folder's `anything2mcp.toml` as layer 6, under the same rules (scope P keys only; a protected key only tightened; a locked key refused), and the request, and the run it starts, use the result. The settings a run used are recorded with the run, by digest. A request that names no project uses no project layer, whichever project started the engine. A setting that takes effect only at a start (the store, the vault, the network, the providers) is refused in a project file, as an installation key is today.

**4. Only a file that only an administrator can write may loosen.** The installation file is read as the installation layer only when the file, and each folder above it up to the root of the installation's configuration directory, can be written by no account but an administrator: the owner is `root`, `SYSTEM`, or the Administrators group, and no other account has write access (the rule `a2m-local`'s `kept_by` already applies to a project folder). A file that fails the check is still read, and every key in it is held to the rules of the user's layer: it may tighten a protected setting and never loosen it, and an installation-only key in it is refused, by name, with the reason. Under `A2M_HOME`, the installation folder is inside the person's own folder and so always fails the check, which is the point.

**5. The person is told where a value came from and which engine holds it.** `config.show` with a project folder shows that project's layer as the requests from it will get it. A command whose project file differs from the one the engine has for it gets nothing to warn about, because point 3 makes them the same; a command whose variables differ keeps the warning it has.

## What this does not change

- The eight layers, their order, presets, and locks.
- Policy as the one decider: settings carried into the policy engine stay its settings (decision [0145](0145-only-the-policy-engine-clears-a-write-and-the-ledger-reads-the-whole-clearance.md)).
- What a target's files may say.

## Consequences

- Section 226's scope table is replaced by the table in point 1, with a column that says what is reachable on one machine today.
- A test of point 3: two projects with different `policy.max_records_per_run` served by one engine, each run refused at its own project's limit, asked of the store. A test of point 4: an installation file the person's own account can write, which loosens `policy.max_records_per_run`, is refused by name; the same file that tightens it is applied.
- Tests that today loosen a protected setting from an installation file under a test's `A2M_HOME`, such as the trust-roots test, need another way in: the presence-bound record of `config.set` once built, or a test seam the builder names and justifies in the slice. The builder may not weaken the check to keep them.
- On Linux and macOS the installation folders, `/etc/anything2mcp` and `/Library/Application Support/anything2mcp`, are an administrator's already. On Windows the check is what makes the folder under the shared program-data folder trustworthy, and it is to be run there before it is called proven.

## Alternatives weighed

- **One engine per project.** Each project's file would then be the start's. It breaks the one engine per person of section 222, its one lock, and its one channel, and doubles every session. Turned down.
- **Warn, as for variables, and keep the start's project file.** Honest, and cheap. It leaves a developer unable to keep two projects with different limits open at once, which is the commonest use of the project layer. Turned down.
- **Refuse to serve a request whose project file differs from the start's.** Safe, and makes every second project unusable until the engine is stopped. Turned down.
- **Leave `A2M_HOME` alone and document that it gives up protection.** The variable is set by tests and by people who keep two setups apart; neither wants the installation layer to become theirs to loosen. Turned down.
