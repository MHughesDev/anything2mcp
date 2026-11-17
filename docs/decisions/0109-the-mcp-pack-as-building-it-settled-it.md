# 0109. The MCP pack as building it settled it

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** choices that [spec section 304](../spec/3-packs/304-the-mcp-pack.md) and the crate's [design](../architecture/crates/a2m-pack-mcp.md) left open or stated in a form the kernel does not take, for package 8.2 of the [roadmap](../plan/roadmap.md). The mapping, the pins, and the pack's own client are [decision 0108](0108-an-upstream-mcp-server-is-reached-only-through-a-pinned-mapping-by-a-client-of-the-packs-own.md).

## Context

The mode `mcp` already exists, from [decision 0005](0005-seven-modes-behind-one-driver-interface.md); this pack needs no record to make it real. The package built the mapping, the driver for reads and writes over HTTP and over a local program, change detection, the discoverer and its drafting rule, and a bearer token for a remote server. Its tests drive the driver through the kernel's handle against [the stand](../../targets/example-mcp/README.md), in both of its forms, and ask the stand what it stored and how many calls it counted.

## Decision

**What the pack registers.**

| Registration | Content |
| --- | --- |
| Mode | `mcp`, archetype request, with the verbs `read`, `list`, `search`, `create`, `update`, `upsert`, `delete`, `download`, `submit`, and `call`. Four reads and one write at a time on a session |
| Step kinds | `call_tool`, and `list_tools`, which the discoverer sends through the sandbox port so that it never holds a connection of its own |
| Surface kind | `mcp`. Its `address` is an `http://` address, or `stdio` for a local server program, which states `program`, `digest`, `args`, `environment`, and `pass_environment`. `revision` chooses the protocol revision; the handshake revision is the default. The program is declared to the plan as the surface's program |
| File kind | `mapping`, in `mappings/`. A mapped tool is the name `surface/tool`, so the kernel refuses one tool mapped twice for a surface |
| Binding fields | `tool`, `surface`, `args`, `timeout_ms`, `settle_ms`, `read_back` with its own `args`, and `lookup` with a `tool` |
| Lint rules | `mcp.tool_unmapped`, `mcp.tool_unpinned`, `mcp.program_unpinned`, `mcp.write_no_readback`, and `mcp.discovery_bound`, errors; `mcp.free_text`, a warning |
| Discoverer, describer, drafting rule | `mcp_tools`, at `discovered`; its describer; one rule from `upstream_tool` to a mapping entry |
| Auth provider | `upstream_mcp` |
| Settings | `max_message_bytes`, 4 MiB by default, and `max_tools`, 500 |

**Identity.** `natural` and `composite`: the mapped read tool is called with the record's key, through the arguments the entry names under `key_args`. `stamped` and `find`: the binding names a `lookup` tool, a list or search tool whose entry names the member that holds its `records` and, for `stamped`, the argument that carries the `stamp`. `none`: nothing is read back. A write tool's entry may name the members of its result that hold a key the server assigned, under `key`.

**What a write does, and how it is verified.**

- The write is one `call_tool` step that commits. The kernel's handle refuses it without the ledger's intent token, and the driver refuses a write-mapped tool from any step that does not commit.
- Its arguments are the step's named values and, where the entry names one, the run's stamp. They must fit the pinned input schema.
- The tool's own error result is `rejected`/`app_validation_error`. A result whose structured content does not fit the pinned output schema is `drift`/`response_schema_mismatch`; after a write, its fate is unknown.
- A connection or a program lost after the call was sent, an answer that is not a response, and a timeout are all reported as unknown. The handle turns an unknown failure on a committing step into `possible_commit`/`no_confirmation`, the contract's code, so the pack does not declare it.
- The record is read back by the mapped read tool, and the engine compares after normalization. The server's success is never the proof. In the stand's test, a write whose answer was dropped after it committed is found by its stamp, through the lookup tool, and the stand counts one write.
- A lookup that finds no record is absent. A lookup that finds more than one is `possible_commit`/`ambiguous_match`, a code section 304 does not list. A lookup by a read tool reports absent when the tool's error text starts with the entry's `absent_error`.

**Connections.** One for each target, environment, surface, session, and epoch of the session's credentials. A connection outlives a run and is reused by the same identity; one of an older epoch is closed when a newer one opens; a run that ends in doubt closes its connection. Two identities never share one.

**Sign-in.** `upstream_mcp` holds one credential under the role `token` and places it in the `Authorization` header as a bearer token, through the lease. A status of 401 is `session_expired`/`needs_login`.

**Discovery.** `mcp_tools` sends one `list_tools` step and calls no tool. It finds one `upstream_tool` for each listed tool, with its digest, how it stands to its pin (`matches`, `changed`, `gone`, `unpinned`, or `unmapped`), and the server's read-only marking; and one `upstream_server`, with the name and version the server reports beside the ones the mapping recorded. The drafting rule writes a mapping entry with the digest and `effect: read` for a tool the server marks read-only, and an open question for any other. It also proposes `untrusted_fields` from the output schema's free-text members, and `result: text` for a tool with no output schema.

## Not built

Each is a named failure (`input`/`not_built`, or the code given), or is simply absent.

- **A local server program on Windows.** The supervisor's foreign link reaches a Windows child through two handles named in an argument, not on its standard streams, and an MCP server program reads and writes its standard streams. On Linux and macOS the two ends are placed on descriptors 0 and 1. The local-program tests run on Unix only.
- **HTTPS.** `reqwest` is built without a TLS back end. An `https://` surface is `input`/`tls_not_enabled`, and the driver's health reports it unavailable.
- **The protocol's authorization flow,** in which a person opens a link. An auth entry that asks for it is refused.
- **A credential for a local server program**, and any credential that is not a request header. The supervisor has no private way to hand one to a program on a foreign link.
- **Answers sent as an event stream.** Such an answer is `unknown_variant`/`unexpected_status`, with its fate unknown.
- **The drafting of a pass-through read operation.** The drafter writes the mapping entry only.
- **A comparison of the server's version at `prepare`.** Discovery reports a changed version as a finding; nothing lowers the bindings' levels yet.
- **The canary, and stopping an idle local program.**
- **The engine loop.** The tests drive the driver through the handle.

## Where the code differs from section 304 and the design

| Section 304 or the design says | Built | Why |
| --- | --- | --- |
| The mapping's `surface` field holds a server address, or a local program by path and digest | `surface` names the surface. The address and the program are fields of the `mcp` surface in `target.yaml` | An environment overrides a surface's address, and the plan declares a surface's program from its fields ([decision 0124](0124-the-plan-declares-a-surfaces-programs-and-a-schema-checks-run-time-values.md)) |
| `prepare` compares each mapped tool's definition with its pin | `prepare` reads the list; each call compares the tool it calls, after reading the list again if the server gave notice | A change to one mapped tool stops the calls to it, not the calls to the others |
| The server's reported version is compared at `prepare` | Compared by discovery | Not built, above |
| Lint codes `mcp_tool_unmapped` and the rest | `mcp.tool_unmapped` and the rest | A pack's lint code is its name, a dot, and the rule |
| `mcp_write_no_readback` is an error unless the operation is `irreversible` | Unless the tool is mapped `irreversible` | The rule reads mappings, and a call to a tool mapped `irreversible` lowers with that effect class |
| A tool that runs free text is refused | The driver refuses a one-string tool whose entry has no `free_text`. `free_text: true` is a warning, and the tool can be called | The compiler cannot see the server's schema, only its digest, so the reviewer's statement is the only thing it can check. Refusing `free_text: true` outright is open |
| Step kinds: `call_tool` | `call_tool` and `list_tools` | Discovery reaches the server only through the sandbox port |
| Binding fields: `tool`, `args` | Also `surface`, `timeout_ms`, `settle_ms`, `read_back`, and `lookup` | A read-back and a lookup are their own calls |
| The failure table | Adds `possible_commit`/`ambiguous_match`, and `input`/`arguments_invalid`, `tls_not_enabled`, `not_built`, and `payload_unreadable`. `no_confirmation` is the contract's code | As above |
| The design names the SDK as the outside dependency | `reqwest` without default features, and a client of the pack's own | Decision 0108 |
| The design's build order marks only the stand as done | Steps 2 to 4 are built, save what is listed above | The design was not updated |

## What the provider contract and the compiler could not express

No kernel edit was made.

| # | The design needs | The kernel has | The pack |
| --- | --- | --- | --- |
| 1 | A local program on its standard streams on every system | A foreign link whose Windows child receives two handles in an argument | Unix only |
| 2 | A local program's credential, handed privately | No private way to give one to a program on a foreign link | Not built |
| 3 | A free-text check at compile time | A mapping that holds a digest, not the schema | A reviewer's `free_text` flag; the driver checks the listed schema at call time |
| 4 | The bindings' levels falling when the server's version changes | A finding, and no way for a driver to lower a binding | Discovery reports it |

## Consequences

- Section 304 and the crate's design owe the edits in the table above, chiefly the mapping's `surface`, the failure table, the lint code names, and the design's dependency line and build order. The lead owes them.
- On Windows the pack reaches remote servers only, and only over plain HTTP, until rows 1 and 2 and TLS are built.
- Hypothesis H-27, a `discovery` mapping, is not yet tested: package E6 comes after this one.

## What would change it

- A supervisor link that places a pipe on a Windows child's standard streams, or a private way to hand a program its credential: rows 1 and 2 would be built in the pack.
- A remote server worth reaching that offers only HTTPS, or only the authorization flow: either becomes the next piece of work.
- A real server whose tools change wording or annotations often enough that per-call pin checks stop useful work; the pin's contents would be revisited under decision 0108.
