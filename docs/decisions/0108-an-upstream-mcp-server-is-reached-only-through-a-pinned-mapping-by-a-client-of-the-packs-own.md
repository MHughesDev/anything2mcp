# 0108. An upstream MCP server is reached only through a pinned mapping, by a client of the pack's own

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Carries out:** the three ideas of [spec section 304](../spec/3-packs/304-the-mcp-pack.md), as the crate `a2m-pack-mcp` builds them. **Replaces:** the line of the crate's [design](../architecture/crates/a2m-pack-mcp.md) that names the protocol SDK as the pack's outside dependency. The rest of what building it settled is in [decision 0109](0109-the-mcp-pack-as-building-it-settled-it.md).

## Context

An upstream MCP server is code this runtime cannot see. It lists its own tools, describes them in its own words, marks some read-only, and may change any of it between two runs. An agent that reached it directly would take those words as instructions and those markings as facts. Section 304 answers with a mapping a developer reviews, a pin on each tool, and verification added on top.

Two things were open when the package started. How a tool's definition is pinned, and so what counts as a change. And what speaks the protocol: the crate's design named the SDK crate, whose client role was "to be confirmed". [Decision 0130](0130-the-mcp-protocol-layer-is-our-own-because-the-sdk-brings-an-unmaintained-crate.md) then found that the SDK brings a crate declared unmaintained, and said the pack could not use its client role either.

## Decision

**Only what a mapping names is reachable.** A mapping file in `mappings/` names, for one `mcp` surface, each upstream tool that may be called, with the effect class the developer judged (`read`, `write`, or `irreversible`) and, for a write, the mapped read tool that reads it back. The driver refuses before sending, as `policy`/`not_granted`: a tool the mapping does not name, a tool whose entry pins no digest, any tool of a mapping whose purpose is `discovery`, and a tool mapped as a write called from a step that does not commit. The compiler refuses the same cases earlier, so the driver's refusal is the second line.

**A tool is pinned by the digest of its definition.** The digest is SHA-256 over one canonical JSON text, object keys in byte order and no spaces, of the tool's name, input schema, output schema, and annotations. The title and the description are not part of it. They are the server's own words, they shape no call and no result the pack checks, and they never reach a caller as anything but untrusted text.

**Drift is found before a call, never after.** The server's tool list is read at every `prepare`, and read again before a call when the server has given notice that its list changed. The called tool's definition is digested and compared with its pin. A tool whose definition differs, or that is gone from the list, stops the call as `drift`/`upstream_tool_changed`, marked as not sent, and the stand counts no call.

**The server's words stay untrusted.** No failure quotes the server; a tool name it chose appears in a message only as its letters, digits, and a few marks, cut at 96 characters. A tool's title and description go into a finding under `untrusted`, and the describer shows them only as untrusted text. A text result reaches a caller as one value whose field type marks it untrusted, and the mapping's `untrusted_fields` name the structured members that hold text people wrote. The server's read-only marking is used only to draft a mapping entry, never to decide an effect.

**A call is checked against the pin, not the server's word.** Its arguments must fit the pinned input schema, and a structured result must fit the pinned output schema, or it is `drift`/`response_schema_mismatch`. The check covers the keywords the pack reads; discovery names the keywords in a schema that it does not check. A tool whose whole input is one free-text string is not called until its mapping entry says, under `free_text`, that a person reviewed it.

**The protocol client is the pack's own.** JSON-RPC 2.0 over one of two links: HTTP to a remote server, or the standard streams of a local server program the supervisor starts. It speaks the two revisions the endpoint serves: 2025-11-25, which starts with `initialize`, and 2026-07-28, which has no handshake and names its revision in every request. The surface states which; nothing is probed, and an answer in another revision is a stop. It lists tools, following cursors up to a bound; calls one tool, once; and tells the server a request is cancelled when the caller stops waiting. Every failure it returns says whether the request left: not sent, answered, or unknown. HTTP is made with `reqwest`, already in the workspace, with its default features off.

## Alternatives weighed

*Passing the upstream's tool list through,* every tool with its own name and description. It is the cheapest overlay and it lost on every rule it touches: nothing a person reviewed decides what is reachable, the server's words reach the agent as if they were instructions ([section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md)), and a write is believed on the server's word.

*Pinning by the server's reported name and version.* It lost because a version is a claim, and a server can change a tool's schema without changing its version. The version is still compared, by discovery, as a finding (decision 0109).

*Putting the description in the digest.* A change of wording would then stop every binding on the tool until it is re-pinned. It lost because the pack never acts on the description and never passes it on as an instruction, so a change in it changes nothing the pins protect.

*The SDK's client role.* It lost twice: decision 0130 removed the SDK from the tree for its unmaintained dependency, and a client that must say of every failure whether the request left has to own the moment a request is written to its link and the reading of every answer.

## Consequences

- The client is this project's to keep current with the protocol, beside the server layer of decision 0130. A new revision, an answer sent as an event stream, or input asked for during a call is written here when a package needs it.
- Every mapped tool has to be re-pinned by a person after any change to its schema or annotations. That is the point: a binding falls silent rather than calling a tool no one reviewed.
- The crate's design still names the SDK as its outside dependency. The lead owes that line.

## What would change it

- A protocol revision that signs or versions a tool's definition itself, so that a pin could be the server's own statement, checked.
- Evidence that description changes in real servers carry changes of behaviour that the schemas do not show. The description would then join the digest.
- A maintained client library that reports, for every failure, whether the request left. Decision 0130's revisit condition covers the server side; this one covers the client.
