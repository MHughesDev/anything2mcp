# 0130. The MCP protocol layer is our own, because the SDK brings an unmaintained crate

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Supersedes:** the parts "The base" and "The seam" of [decision 0071](0071-the-mcp-server-stands-on-the-official-sdk-behind-a-seam-and-serves-two-revisions.md), which chose the SDK crate `rmcp`. The revisions served, the rule for agreeing on one, and the stdio proxy of that record stand. It also replaces what [decision 0115](0115-the-mcp-endpoint-as-built-checks-in-front-of-the-sdk-and-a-redactor-on-the-way-out.md) says of the SDK: its point 1 now reads "in front of the protocol layer", its point 9 keeps `futures-core` for a different body, and its consequence about what the SDK logs no longer applies.

## Context

The advisory check could first be run on a tree that held `a2m-mcp`. It refused it: `chrono` 0.4.45 is declared unmaintained (RUSTSEC-2026-0543, all versions affected, no safe upgrade), and `rmcp` 3.10.0 depends on it.

What was checked before leaving the SDK:

- **A newer release.** 3.10.0, of 2026-11-12, is the newest on crates.io.
- **A set of features without it.** There is none. `chrono` is a plain dependency of `rmcp` for every target, not an optional one, and `rmcp` also turns on the `chrono04` feature of `schemars`, which its `server` feature needs.
- **An ignore entry.** This repository does not silence an advisory; [decision 0123](0123-the-linux-secret-service-back-end-is-left-out-until-its-store-crate-is-maintained.md) removed a back end for the same reason.

Decision 0071 named the way out: a layer of our own behind the same list of messages. By then `a2m-mcp` already held the checks in front of the SDK and the redactor behind it, and the list of messages was a test with raw messages on both transports.

## Decision

**`a2m-mcp` does not depend on `rmcp`.** The protocol between the checks and the dispatcher is one module of this crate, written with `serde_json`, `axum`, and Tokio:

- The methods: `initialize` and `ping` of revision 2025-11-25, `server/discover` of revision 2026-07-28, and `tools/list` and `tools/call` in both. Anything else is -32601.
- The result shapes of both revisions: `resultType`, `ttlMs`, `cacheScope`, and the server's name in `_meta` for the stateless revision, and none of them for the handshake revision.
- The rules for a request that names its revision: its three headers must say what its body says, or the answer is status 400 with -32020; the client's capabilities must be in its `_meta`, or the answer is -32602.
- A tool call whose caller asked for progress is answered as a stream of events: its progress, then its response. Any other request is answered with one JSON message. When the stream is dropped, the endpoint stops waiting; the dispatch itself is never cut off.
- The content types: a request that is not `application/json` is status 415, and one whose caller does not accept both kinds of answer is status 406.

The public interface of `a2m-mcp` did not change. Every test of decision 0115 passes unchanged, except the one about what the SDK logs: it now holds this crate to logging nothing that passed through the door.

**One outside crate stays:** `futures-core`, for the stream trait that the event stream's body needs. It is in the tree of `axum` and is maintained.

*Instead of:* waiting for an SDK release without `chrono`. Nothing says when one comes, and the pull request cannot merge until then.

*Instead of:* an ignore entry for the advisory with a date. The repository's rule is that the check holds the gate.

## What the SDK did that this layer does not

None of these was promised by the design's list of messages.

- Keep-alive comments on an open event stream. A call held open for minutes through an intermediary that closes idle connections would need them.
- A check of the `Origin` header. It was off by default in the SDK too; the `Host` check and the caller check stand.
- Typed parsing of every request the protocol knows. This layer reads the fields it uses and checks their types; a method it does not serve is an unknown method whatever its parameters.
- The methods the SDK answered with an empty result by default, such as `prompts/list`, `resources/list`, and `logging/setLevel`. They are unknown methods here, and no capability names them.
- Its log events, which carried the caller's input at the debug and trace levels.
- Sessions, the stream a client opens with `GET`, resumption, and everything of the client role. None was used.

## Consequences

- The tree loses `rmcp`, `chrono`, `schemars`, and the crates only they brought. `cargo-deny check` passes all four parts.
- Keeping up with the protocol is this project's work. A new revision, round trips for input, tasks, and subscriptions are written here when their packages build them, against the protocol's text, with a line in the list of messages for each.
- The two reference client libraries have not been run against this layer. The spike ran them against its own hand-written server over stdio only. Running both, over HTTP and through the proxy, is owed before Phase 1 exits.
- The MCP pack, which speaks to existing MCP servers as a client, cannot use the SDK's client role either.

## Revisit if

- A release of the SDK has no unmaintained crate in its tree. Then the question of decision 0071 is open again, with the cost of keeping this layer current as the evidence.
- A client library in the conformance set fails against this layer where it passed against the SDK.

## Spec sections

- [222. Front doors and the local engine](../spec/2-kernel/222-front-doors-and-the-local-engine.md)
- [605. Packaging and release](../spec/6-running-it/605-packaging-and-release.md)
