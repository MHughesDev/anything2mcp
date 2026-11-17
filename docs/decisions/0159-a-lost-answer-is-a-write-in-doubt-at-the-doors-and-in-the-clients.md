# 0159. A lost answer is a write in doubt at the doors and in the clients

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

## Context

A safety audit of the doors and the bindings found that a call whose answer was lost after the engine had it was reported as safe to make again. The stdio proxy answered -32000 with the local channel's fix, "Run the command again", and both SDKs said "Run it again." An agent that does so starts a second run of a write. It found smaller gaps beside it: unbounded reads in the proxy, streams that wait for ever on a reader that stops reading, a Python client that reconnected with no owner check and no hello, an npm launcher that ran any package of the platform package's name found in a folder above it and passed no signal on, and action names put into a request's path unchecked. The action registry was also asked for three things: a follower woken at once when its run came to rest between two calls, the built-in guides served by `docs.get`, and `McpResultFace::of_in` used by the MCP door.

## Decision

Each finding was checked against the code with a test that failed before the fix.

| Finding | Confirmed | Fix | Alternative weighed |
| --- | --- | --- | --- |
| M1, the proxy (serious) | Yes | A failure of class `busy`, `policy`, `input`, or `transient` from the upstream means nothing was sent: -32000 with `retry_safe` true. Any other class, and a `tools/call` whose answer broke off, came back with a status of 500, or carried only notes of progress, is a write in doubt: `data` of class `possible_commit`, code `answer_lost`, `retry_safe` false, and a fix that says not to call again and to find the run first. A request that is not a tool call writes nothing and stays `retry_safe` true. A stream's call is answered by its response, never by a note, so a stream that breaks after a note is still answered | Classifying by the local channel's codes. The proxy does not depend on `a2m-local`; the class is the contract, written on `Upstream` |
| M2 (minor) | Yes | A line is read to at most 4 MiB, the endpoint's cap; a longer one is answered -32600 and dropped as it is read. At most 64 requests are carried at once; the next line waits | Refusing requests beyond the limit with an error. That would drop notifications; waiting does not |
| M3, A2 (minor) | Yes | Each message of a stream waits at most 60 s for its reader (`READER_PATIENCE`); then the reader is treated as gone. Following only reads, and a dispatch runs on its own task | No bound, relying on the client to close. A connection left open would hold a follower for ever |
| A1 (minor) | No route to harm | A paragraph in the REST door's design: it walks nothing through the redactor because everything it sends was redacted before it was handed over and its own texts quote no input; a route that comes to quote input must add the pass | Adding the pass now. It would be a second walk over every answer for nothing that can reach it today |
| B1, the SDKs (serious) | Yes | `ChannelError` carries `retry_safe`. A request on a closed connection is refused before a byte is written: `connection_lost`, `retry_safe` true. Once the request may have been written, a lost, broken, oversized, or unreadable answer, and the caller's own `timeout`, are `answer_lost`, `retry_safe` false, with the same fix as the proxy's | One code for both, as before. The caller could not tell a safe retry from an unsafe one |
| B2 (minor) | Yes | The Python connections open once; `http.client` raises `NotConnected` before sending on a closed one, and the client says so | Re-running the owner check and the hello in `connect()`. A reconnect behind the caller's back would hide that the engine went away |
| B3 (minor) | Yes | The launcher looks for the platform package only inside its own folder and beside it in the `node_modules` folder that holds it, or through Node's resolution under Yarn's Plug'n'Play. The package's name and version must be the launcher's, and the program's resolved path must lie inside the package. The Python `a2m` already starts only the binary beside its resolved path; `a2m.cmd` now first checks that its folder holds `a2m.cmd`, because cmd can give the working folder as `%~dp0` | `require.resolve` with a version check. It would still run a planted package of the right version from any folder above |
| B4 (minor) | Yes | `spawn` in place of `spawnSync`; `SIGINT`, `SIGTERM`, and `SIGHUP` are passed on, and the launcher ends as the program ends. On Windows it only waits | Exiting the launcher and leaving the program running, as before |
| B5 (minor) | Yes, no reachable harm | Both clients refuse an action name that is not `noun.verb` before a byte is sent | Checking against the generated list of actions. An engine newer than the client would then be unreachable for its new actions |
| The follower's race | Yes: the audit test waited out 120 s in the full suite | `Kernel::next_events_from` takes a `Followed`, the last event's number and whether the run was settled then, and answers at once when that has changed. `next_events` keeps its signature and calls it with nothing known. Both doors follow with it and their 20 ms doubling look is gone | A timer, as the doors had. The notice that a job ended already reaches the kernel's watch; only what the follower had seen was missing |
| The guides | Built | `docs.get` serves `guides/*.md` first, by the name on their first line, and ends each page with whether each action it names is built, read from the registry when asked. The in-code topics `failures` and `approvals` are gone | Writing the built state into the files. It would go stale with the next action built |
| `of_in`, instructions | Built; instructions unchanged | The MCP door renders with `McpResultFace::of_in` and its toolset. Section 221 says nothing of server instructions, so they stay one sentence; section 220's generation from the guidance file remains not built | Serving the whole guidance page as instructions. Section 220 asks for the handful of rules every caller needs, not the guide |

**Idempotency keys.** The REST door and both SDKs carry an `Idempotency-Key` to the kernel, and no handler reads it: a second call with the same key is tied to the first one's run only while that run is unfinished, and then by its input, as every call is. So no client is made to send a key: it would not make a repeated call safe. The clients say so where the key is offered. An MCP request carries no key at all. For a key to turn an unsafe retry into a safe one, `tool.call` must fold the key into a run's identity, finished runs included; that is owed by the crates that own the run.

## Consequences

- A caller told `answer_lost` is sent to its recent runs. `run.list` is not built, so today the fix also says to tell the person.
- The local channel's codes now match: `connection_lost` (class `transient`) means not sent, and `answer_lost` (class `possible_commit`) means the engine may have had it.
- The code `answer_lost` is added to the technical reference under `possible_commit`.
