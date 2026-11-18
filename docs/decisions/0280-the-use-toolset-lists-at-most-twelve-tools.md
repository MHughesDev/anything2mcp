# 0280. The `use` toolset lists at most twelve tools

**Status:** Accepted. A design decision by an agent, made on 2026-11-18; any agent may supersede it with evidence. The owner has not read this record.

**Date:** 2026-11-18

**Supersedes in part** [decision 0094](0094-the-action-layers-interface-as-building-it-settled-it.md), point 3, its last sentence, which gives the `use` toolset a cap of 19. The rest of point 3 stands. **Settles** what [decision 0260](0260-the-machines-network-facts-are-served-as-reads-of-the-local-user-one-listed.md), point 3, left open.

## Context

A built-in toolset lists its tools up front, to every agent that connects, and reaches the rest of its actions through `a2m_action_run`. Decision 0094 settled that a toolset's cap limits what is listed and not what may be marked, and gave the caps as 15 for `build` and 19 for `use`. The code has given 12 for `use` since it was first written: `BuiltInToolset::cap` in `crates/a2m-actions/src/def.rs`, which `ActionRegistry::toolset` in `registry.rs` applies after the three `action.*` tools. Decision 0260 saw the difference, worked to the code, and left it to a record of its own.

The number 19 is also the default of the setting `tools.listed_limit` ([section 226](../spec/2-kernel/226-configuration.md); `McpSettings::local` in `crates/a2m-mcp/src/settings.rs`): the most tools a list holds when a target's published pipelines are counted in with the built-in tools. Whether decision 0094 meant that setting cannot be told from the record. The toolset's cap and the list's limit are two numbers, and both are in force.

Every tool listed is paid for in every conversation, before the agent has asked for anything. Round 2 of the [idea rounds](../plan/idea-rounds.md) measured the list the bridge sends: the `use` toolset's eleven tools were 8,431 bytes, about 2,100 tokens, and the largest single tool 1,159 bytes.

## Decision

1. **The `use` toolset lists at most twelve tools.** They are, in catalog order: `a2m_tool_search`, `a2m_tool_describe`, `a2m_tool_call`, `a2m_run_get`, `a2m_run_resume`, `a2m_run_cancel`, `a2m_session_status`, `a2m_session_login`, `a2m_net_standing`, `a2m_action_search`, `a2m_action_describe`, and `a2m_action_run`. With `a2m_net_standing`, listed by decision 0260, the toolset is full. The cap of the `build` toolset stays 15.
2. **Every other action is reached through `a2m_action_run`,** found by `a2m_action_search` and described by `a2m_action_describe`. Its own role, schema, and policy decision apply there as they would to a listed tool. An action marked for `use` past the cap is left out of the list and counted by `doctor.run`, as decision 0094 settled.
3. **The watch tools are not listed.** No `watch.*` action is in the catalog yet: the registry half of [decision 0262](0262-watches-are-held-to-their-end-their-budget-and-their-runs-own-result.md) is not built. When it is built, its actions are reached through `a2m_action_run`, unless a record gives one of them a place by taking one from another tool or by changing this cap.
4. **The list's limit is unchanged.** `tools.listed_limit`, default 19, counts the twelve built-in tools of `use`, so seven places are left for a target's published pipelines by default; `tools.full_listing` lists every published tool for a client with tool search of its own.

## Consequences

- The code and this record agree; no code changes.
- Adding a tool to the `use` list is now a choice between tools, made by a record that says which tool leaves the list or why the cap grows.

## Alternatives weighed

- **A cap of 19, as decision 0094 states.** Seven more tools in every agent's list, each paid for in every conversation, for actions an agent reaches in one call through `a2m_action_run`. Not taken.
- **A smaller list: search and call only.** Round 2 ruled it out: it would save little and cost a search before every call.
- **Make the cap a setting.** `tools.listed_limit` and `tools.full_listing` already let a person change how much a client sees; a second setting for the built-in part would be a second place to get it wrong.

## Revisit if

Agents are seen to miss an action they need because it is reached only through `a2m_action_run`; an action is built that agents need before any of the twelve; the twelve tools are measured again and their list is larger than the 10,424 bytes round 2 found for the fifteen of `build`; or the clients most used come to search their tools themselves, so that the list's size stops costing a conversation.
