# 0999. A tool's name carries no version, and a breaking change is named at publish

**Status:** Accepted. Design decision, made in the design review round by lens 14, a year later ([report](../reviews/a-year-later-review-2026-11-25.md), finding 1). The owner had not read this record when it was written. It settles how a tool's contract changes between published versions, where three documents said three things. It changes no built code; the parts it promises are roadmap packages YL1 and YL2.

**Date:** 2026-11-25

**Changes:** [section 206](../spec/2-kernel/206-the-compiler-and-the-compiled-target.md) ("Compatibility"); [section 221](../spec/2-kernel/221-the-tool-surface-agents-see.md) (names, the result's example, what a tool's `_meta` carries); [section 604](../spec/6-running-it/604-management-and-operations.md) (the gap review's row on tool contract versioning); `version.diff` and the review summary once YL1 is built.

## Context

A tool's name is its operation's name, `{target}_{module}_{object}_{verb}`, and an agent and its person keep it: in a prompt, a saved plan, a client's list of allowed tools, a script. Once the first release ships, that name and the tool's schemas are what people depend on most and can least see change.

The documents disagreed on how a tool's contract changes:

- Section 206 said a breaking change "requires a new major number in the tool's name".
- Section 221's example result named the tool `example_order_sheet_enter@4`. No MCP tool name may hold `@`, and the code returns the bare name (`tool_name` in `a2m-actions/src/handlers/tools.rs`).
- The compiler refuses two tools of one target whose names differ only by a suffix such as `_v2` or `_new` (`kernel.tool_names_too_alike`), and its fix says "a breaking change is a new major number, published as a new version": the number is the target's version, not a part of the name.
- Section 604 said toolsets pin versions, old versions warn before removal, and a new version can go to named callers first. That is a server's design; on one machine there is one person and one current version.

What is built: each listed tool carries its target and version in `_meta` (`anything2mcp/target`, `anything2mcp/version`). What is not built: `version.diff`, which section 206 says classifies each change; `anything2mcp/digest`, which section 221 says lets a client pin a tool and notice a change; and any part of the review that compares a version with the one before. So today a person can publish a version that removes an input, and an agent's next call fails as `input`/`schema_invalid` with no earlier word to anyone.

## Decision

1. **A tool's name never carries a version.** The name is the tool's identity across every version of its target. The version is in `_meta` as `anything2mcp/version`, and what the tool is, exactly, is in `anything2mcp/digest`: the SHA-256 of its name, description, input schema, output schema, and annotations in canonical JSON. A client that pins a tool pins its digest.
2. **What is breaking is a closed list,** compared between the current published version and the one being published, tool by tool: the tool is gone; an input is gone or renamed; an input becomes required, or a required input is added with no default; an input's type changes or its words narrow; an output is gone, or one that was always present may now be absent; an output's type changes; the effect class rises; the hazard is added or changed; the record's key changes. Anything else is compatible: a new tool, a new optional input, a new output, wider words, a changed description. A change to a description alone is compatible and still changes the digest.
3. **The person who publishes is shown it.** The review summary of `publish.request` lists first, before the sign-ins, each tool whose change is breaking and why, by the list of point 2; the publish is not refused. On one machine the person who publishes is the person whose agents call the tools, and is the one to judge; `version.diff` gives the same classification for any two versions.
4. **To keep the old shape beside the new,** a person publishes a second operation with a name that says what differs, which the existing lint already requires. No suffix of a number is a way to version a tool.
5. **Pinning by callers, warnings before removal, and a staged roll-out to named callers** stay the server's (section 604), after the first release.

## Alternatives

| Alternative | Why not |
| --- | --- |
| A major number in the name, as section 206 said (`example_order_get_v2`) | The agent's person must rename the tool in every prompt and list for every break, and the old name either stays, doubling the list an agent chooses from, or goes, which is the break again. The lint refuses it for the reason that agents cannot tell such names apart |
| Refuse a breaking publish unless the person names it | Asks the person to say what the page already shows them. The person sees each break by name; refusing adds a step and protects nothing more on one machine |
| Keep the previous version callable beside the current one | A server's feature (section 604); on one machine it doubles what is listed, and the old version's sign-ins and addresses stay live |

## Proof

Package YL1: two versions of the test target in `a2m-actions`' tests, the second removing an input, adding a required one, raising the effect class, and removing an output, each in a tool of its own, plus one tool given a new optional input. `version.diff` names each tool with its class and reason; the review summary lists the four breaking ones first and the compatible one with the rest. Package YL2: the digest is in each listed tool's `_meta` and in `tool.describe`, it is the same on three operating systems for the example target, and it changes when a description changes and not when the version number alone does.

## What would reopen it

A server, where a toolset pins a version for callers who are not its publisher; or evidence that clients cache tools by name and cannot see a changed schema, which would make the name the only signal a client reads.
