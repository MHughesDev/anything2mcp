# 0399. The `site_tools` mode calls a tool a page registers, by its name and digest, as a write that cannot be taken back

**Status:** Accepted. Design decision. The owner had not read this record when it was written. Carries out package 8.2a, on the reading of [research note 25](../research/25-webmcp-standard-status.md).

**Date:** 2026-11-24

**Changes:** the web pack's manifest (a mode, `site_tools`, with the steps `goto`, `wait`, and `call_tool`, and the failure codes `site_tool_not_offered`, `site_tool_changed`, and `site_tools_unavailable`); the browser base (`ActTab::site_tool`, `ActTab::call_site_tool`, `SiteTool`, `SiteAnswer`); the binding of the mode in [file formats](../reference/file-formats.md); [spec section 301](../spec/3-packs/301-the-web-pack.md); the test application's plan (a page that registers tools).

## Context

A page may register tools for an agent in its own browser, with `document.modelContext.registerTool`, and Chrome lets a client list and call them through an experimental DevTools domain, `WebMCP` (note 25). The proposal is a draft, the domain is experimental, and what a tool does is whatever the page's script does. Package 8.2a is the mode that calls such a tool.

## Decision

1. **The mode is the web pack's, and one driver serves it with `ui`.** `site_tools` has the archetype `request` and the steps `goto`, `wait`, and `call_tool`. It shares the driver of `ui`, so the two share a browser, a tab's origins, the refusal of the edge, and the checks that a page that moves, opens a window, or offers a download stops the run. A binding is `mode: site_tools`, with the verb `call` or `submit`, a `screen` (which page to be on, and when it is ready), a `tool` (its name), a `digest`, `args` (a map from an argument's name to an expression), and `result` (the output that takes the tool's text).
2. **A tool is bound by its name and a digest.** The digest is that of the tool's name, its description, and its input schema, each with its length. Before a call the driver waits (up to five seconds) for the browser to announce the tool, and compares: a tool that is not announced is `site_tool_not_offered`, and one whose digest differs is `site_tool_changed`, both class `drift`, with nothing sent; the second says which digest the page's tool has, so a person who looked at the tool can pin it. The tool's description is the page's text: it is hashed and never shown to an agent as an instruction.
3. **A call is a write that cannot be taken back, whatever the page says.** The mode's least effect is `irreversible`, its `call_tool` step is the commit point (`commits` always), and its evidence is the page's own acknowledgement, so the engine ends such a record `acted` and never `verified`. The page's hint that a tool only reads is its own claim; it is kept on the announced tool as a claim and sets nothing. An operation that says it reads does not compile over such a binding. A read-back of another mode, which would make the write verifiable, is a later binding, not this mode's.
4. **The call runs in an acting tab, once, and its end may be unknown.** A run that may not act does not call a tool (`read_only_posture`, nothing sent). The call is sent once. An answer that is an error or is cancelled is `possible_commit`/`no_confirmation` with the fate unknown, since the page's script may have done part of what it does; an answer that does not arrive within the step's time is cancelled and ends the same way. A browser with no such domain stops with `site_tools_unavailable`, with nothing sent.
5. **The arguments are held to the tool's own schema, and the answer is text.** The binding's arguments are sent as the object the schema describes: no name the schema does not list, every name it requires, and each of the type it names (a number is a number, an exact decimal is its text). An answer is the text of the tool's content, or what it said, bounded at 256 KiB, and it is the value of the `result` output. The output must be of the type `untrusted_text`: the page's answer is never trusted.
6. **What the proposal changes is behind one private interface.** The base's three calls and two types are the only place the domain's names appear; a protocol change is a change there. The browser the engine tests with (Chrome for Testing 153) needs the frame of the tool and a JSON object as the input, which the base supplies.

## Not built

The declarative form tools, tools of a service worker, tools of a frame of another origin, a tool called while its tab is in the background, the draft of a `site_tools` binding from a page's registered tools (the `declared_tool` finding), and a read-back through another mode. The digest is written by a person, copied from the stop that `site_tool_changed` gives.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Call `document.modelContext.executeTool()` from a script the engine puts in the page | It puts engine code in the page's world, is bound by the page's own origin rules, and gives no events to wait on (note 25) |
| Bind by the name alone | A page can change what a tool does between visits; the digest turns that into a stop |
| Take the page's `readOnly` hint as a read | The page may omit or misrepresent it, and the proposal's own text says so |
| A driver of its own for the mode | The two modes share a browser, tabs, and the checks above; a second driver would hold the same state twice |

## Proof

In `crates/a2m-pack-web-ui/tests/site_tools.rs`, against the page `/cases/site-tools` of the test application: a digest that is not the tool's stops with the right digest in the message and tells the page nothing; the right digest calls the tool, which tells the page once, and its answer is the output; a tool the page marks as only reading is called and is still held to `irreversible`; a tool that fails is `no_confirmation` with the fate unknown; a tool the page never registered is `site_tool_not_offered`; a tool the page registers a moment after it loads is waited for; a run that may not act calls none. Through the engine loop, in `crates/anything2mcp/tests/site_tools.rs` over the project `tests/projects/site_tools`: the version is published after its offline test passes, the tool is found as `irreversible`, an agent's call waits for approval and tells the page nothing, the person approves, the resume tells the page once, the answer is the output, and the trace shows one intent and no `verified`.

## What would reopen it

The domain leaving its experimental status, or a stable identifier for a tool that makes the name and the digest unnecessary; or a site whose tools cannot be told apart by name.
