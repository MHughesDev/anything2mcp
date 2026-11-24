# 0340. The doors say who wrote a description, the scout report lists what it did not draft, and a person counts bindings by the file

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-23

**Changes:** `tool.search` and `tool.describe` (the field `description_by`), the `_meta` of a published tool at the MCP door (`anything2mcp/description_by`), the scout's draft stage and report (`not_drafted`, `left_out_as_secret`), the places `a2m-actions` reads and writes a binding by, and the help of `secret.set`. **Leaves alone:** the compiler, the plan, the provider registry, and the toolsets the MCP door serves.

## Context

[Decision 0337](0337-a-description-says-who-wrote-it-a-drafter-reports-what-it-left-undrafted-and-bindings-are-ordered-by-rank.md) built the compiler's halves of three packages: a tool of a plan carries who wrote its description; a drafter reports the findings it drafted nothing from and the properties it left out as a secret's; the compiler orders an operation's bindings by the rank of each binding's mode. This record is the other half in the action registry and the MCP door.

## Decisions

1. **The two doors that describe a published tool say who wrote its description.** `tool.search` (when it lists in full) and `tool.describe` return `description_by`, `rule` or `builder`. The MCP door adds `anything2mcp/description_by` to the `_meta` of a published tool, beside the target and the version. A control tool has no `_meta`, as before. (Roadmap V5.)
2. **The scout's draft stage lists what it did not draft.** Its result carries `not_drafted`, one entry per finding a rule covers and drafted nothing from, as `{finding, description, pack, reason}`, and `left_out_as_secret`, the count of properties and parameters left out by name. `description` is the finding's own words, found by its id, and marked untrusted as every word the system gave. A rule that gave no reason is worded "the rule gave no reason". The report's text names both counts, and `scout.report` pages the list under `drafted.not_drafted`. (Roadmap FT11.)
3. **A person counts a binding by the file; the engine counts it by the plan.** Once the compiler orders bindings by rank, an operation's file and its plan list its bindings in different orders. Wherever a person reads or writes a place (a `position` in a call, the place `binding.promote` rewrites, the `binding:` a recorded test file names, the place an advice tells a person to write), `a2m-actions` uses the file's place. Wherever it reads the plan (a pin, a run, a test's evidence), it uses the plan's place. The two are converted by the bindings' modes alone: a mode has one rank, so the bindings of one mode keep the file's order among themselves, and the k-th binding of a mode in the file is the k-th of that mode in the plan. The conversion refuses to guess when the two lists differ in length. (Roadmap 4.6, in part.)
4. **`secret.set`'s help states the form it accepts.** A name is one to four parts joined by `/`, each of lowercase letters, digits, `_`, `-`, and `.`, at most 64 characters a part, and a target's `auth.yaml` takes only a secret whose first part is the target's name. (Field finding 14.)
5. **The MCP door still serves only `use` and `build`.** A toolset a policy set defines is not given an endpoint yet. The alternative, and why it was not built now, are below.

## Alternative for the fifth decision

Serving a defined toolset needs more than a name accepted by `McpService::new`. It needs (a) the set in force read when a client connects, since a set changes while an engine runs, and a name the set does not define refused (unknown means stop); (b) a list of actions for a toolset the engine does not know, which no document states; (c) the listener above the door, in the product crate, to route `/mcp/<name>` for names it does not know at start, and the stdio proxy to name one; and (d) the policy engine to narrow the list of published tools by the toolset, which decisions already do for a call but the list does not. Each is a design of its own and none is small. A person who needs a narrower toolset today has the grants and `use`, and a client that is shown a tool it may not call is told so by the decision. It is built when a first use of a custom toolset's endpoint states what its action list is.

## What this risks

A place converted by modes alone is right while a mode has one rank, which the manifest states; a manifest that gave one mode two ranks is refused by the compiler's own check. The conversion of a test file's own `position` is the compiler's and was built in 0337; here only what a person is shown and writes is converted.

## What was not built

The provider registry does not yet hand each mode's rank to the compiler (`CompilerSetBuilder::mode_rank`), so no plan is ordered by rank in a real engine, and the proof of package 4.6 (a read gains a request binding through shadow, is promoted, and the next live call goes through it with no hand edit) is not met. Roadmap 4.6 stays `ready`, with the places built and the registry's call as the one piece left.
