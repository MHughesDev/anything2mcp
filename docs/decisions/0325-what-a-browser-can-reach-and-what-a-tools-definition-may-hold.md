# 0325. What a browser can reach, and what a tool's definition may hold

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Supersedes point 6 of [decision 0307](0307-what-the-evidence-level-is-for-and-what-a-review-shows-about-a-tools-evidence.md) (package V5) in part: its bound now covers the whole definition, and the rest of that point stands.

**Date:** 2026-11-23

**Changes:** [section 222](../spec/2-kernel/222-front-doors-and-the-local-engine.md) (local HTTP, when a list changes, the stdio proxy's output); [section 221](../spec/2-kernel/221-the-tool-surface-agents-see.md) (bounds, the tool digest, untrusted fields); [section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md) (reaching the engine through a browser, the bound, the linter, what this cannot do).

## Context

[Research note 39](../research/39-mcp-transport-and-injection-threats.md) read four pages of the MCP specification from its repository, through a summariser, and one advisory in full; what it has on tool poisoning, changed definitions, and shadowing is from search summaries. The specification's rule for a local HTTP server, read there, is to bind to loopback, check `Origin` and answer 403, and authenticate; the advisory shows a server SDK that left the protection off by default. The product's local channel is a socket or a named pipe and opens no TCP port, and its one loopback listener already checks `Host` and `Origin`. What was not written down is a rule for local HTTP, which the workstation profile switches on, and what a tool's definition may hold beyond its description. The attacks on a client that loads definitions from a server it does not control rest on summaries; the rules below do not depend on how common they are, only on what a definition is.

## Decision

1. **Local HTTP** binds a loopback address only; answers 403 to a request whose `Host` is not a loopback name with the listener's own port, or that carries an `Origin` other than its own, before any credential is read; lets a request with no `Origin` go on to the credential check; needs a bearer credential on every request, issued to the person's own client files and readable by the person alone, never in a query string; and does not start without a credential check, with a failure code declared in the local engine's table when it is built.
2. **The definition is bounded, not only the description.** Every free-text field of a tool's definition (its description, each field description in its input and output schemas, enum labels, and defaults of text type) is at most 1,024 characters, and a definition holds at most 8,192 in all. The characters that [decision 0193](0193-the-core-the-contract-the-compiler-and-the-scheduler-after-the-second-pass.md) writes as escapes in a result are refused in them: control characters, bidirectional marks and isolates, invisible format characters, and line and paragraph separators. A newline and a tab are allowed in a description. The compiler refuses what breaks them.
3. **A tool carries its digest.** Each listed tool carries, in its `_meta` under `anything2mcp/digest`, the SHA-256 of its name, description, input schema, output schema, and annotations in canonical JSON, so a client can pin a tool and see that it changed.
4. **A list changes only by a person's act.** Publishing a version, switching an environment's version, or changing a toolset's members; a changed description is always a new version. Where a connection can carry a notification, the door declares `listChanged` and sends it on each change.
5. **The system's words are marked in the output schema.** Each field that holds them carries `x-anything2mcp-untrusted: true`.
6. **The stdio proxy writes only complete MCP messages to standard output,** and diagnostics to standard error only.
7. **What stays outside is said.** Anything2MCP is never an MCP client; a hostile server beside it in the same agent can steer the agent's use of its tools, and only the agent's host can stop that. Nothing in the product relies on a client honoring its annotations.

## Alternatives

| Alternative | Why not |
| --- | --- |
| No local HTTP at all | The workstation profile is designed with it; the rules make it as safe as the channel's own check allows |
| Bound the description only (decision 0307 point 6) | Field descriptions, enum labels, and defaults reach the model as text just as the description does |
| A notification as the only sign of a changed list | The stateless revision cannot always carry one; a digest works on every revision |

## Proof

The compiler refuses a definition with a 1,025-character field description, one over 8,192 in all, and one with a bidirectional mark in an enum label, and accepts a description with a newline. A listed tool's digest changes when its description changes and not otherwise. Package 6.2 and local HTTP, when built: a foreign `Origin` and an unlisted `Host` are refused with 403 before a token is read, and a listener without a credential check does not start. The stdio proxy, started with a failing engine, writes nothing to standard output that is not a whole message.

## Owed to files this record does not edit

Roadmap package V5's row: its bound covers the whole definition as point 2 says, and its proof is this record's first sentence of proof. Section 601's workstation row: that local HTTP is the one place in the first release a web page's script could reach, held to section 222. Roadmap package 6.2's proof: a token for another audience, with no audience, and with the metadata address as audience are each refused with 401; a foreign `Origin` or an unlisted `Host` is refused with 403 before any token is read; a 401 carries the `resource_metadata` address.
