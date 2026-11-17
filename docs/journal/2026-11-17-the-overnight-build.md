# 2026-11-17: the overnight build

This entry is written while the build is under way, and is brought up to date as each wave lands. Its last section says what was true when the session ended.

## State at the end

*To be completed when the session ends. Until then, the sections below are current.*

## Done

The owner asked for the whole system to be built end to end, without stopping, by as many agents as could work at once. [Decision 0067](../decisions/0067-the-kernel-is-built-layer-by-layer-in-parallel-and-the-first-driver-is-the-http-packs.md) records how: the kernel layer by layer, one agent per crate, each against the crate's written design; the first driver through the loop is the HTTP pack's; test stands, the harness, and the spikes beside the kernel from the first hour.

**Wave 0 and 1: types, foundations, stands, spikes.**

- `a2m-core`, complete: values with exact decimals, checked names, identifiers, the plan as canonical JSON with its hash check, redaction, keyed digests (decisions 0068, 0069). A second pass added what the run time needs from target files, a public macro for checked names, and wiping a secret when it is dropped (0121, 0122).
- `a2m-expr`: no maintained CEL crate can refuse a floating-point number when an expression is checked, so the language is a typed subset of CEL on our own parser, checker, and evaluator. Decimals use operators; division is a function that states its rounding (0072, 0073).
- `a2m-vault`: sealing under a key hierarchy, key stores for the three operating systems and a passphrase route, key records, and the redactor with encoded forms (0074, 0075).
- `a2m-telemetry`: one trace record per step, a signature chained over record digests, logs through our own subscriber, spans and metrics through a handle (0076, 0077).
- Six test stands, each a workspace crate under `targets/` with a control interface that reports ground truth and faults as settings: a command-line program, a database, files, a broker, a simulated device speaking a subset of the Modbus application protocol, and an MCP server.
- The benchmark harness, with one sample scenario, run in CI.
- The browser spike: the engine stands on a DevTools-protocol client of our own over the debugging pipe; every outside crate was refused for a stated reason (0070, research note 17). The MCP spike: the service stands on the official SDK behind a seam, serves two protocol revisions without a session, and was called by two unrelated client libraries (0071, research note 18).

**Wave 2 and 3: stores and services.**

- `a2m-files`: target files read by an event parser into our own located tree, checked by our own schema subset, compiled to a plan through registration points that a pack fills; the drafter's kernel half; the `example` target (0078, 0079).
- `a2m-ledger`: one SQLite file; runs, events, the ledger with its nine rules enforced in the calls and again by triggers, record locks, intent tokens with no public constructor, blobs, findings, published versions, the audit chain. A crate above reaches its own tables through the transaction, and the database refuses a statement that names another crate's table (0080, 0081).
- `a2m-policy`: Cedar was measured and refused (a licence outside the closed list in its build tree, 107 crates, 5.8 MB), so the decision core is the crate's own evaluator; approvals exist only on a handle that `open` returns once (0082 supersedes 0033; 0083).
- `a2m-auth`: the session keeper with one renewer, leases, the auth provider contract, the neutral providers, saved state sealed (0084, 0085).
- `a2m-sources`: csv, json, and xlsx readers of our own over one outside crate that inflates; exact decimals through all three; template fingerprints (0086, 0087).
- `a2m-sched`: admission, queues with round robin between callers, leases with fencing through the ledger, exclusive resources with an ordering rule, budgets by the store's clock (0093).

Pull request 11 carries everything above. About 600 tests pass on Linux.

## Learned

- **The designs were ready to be built in parallel, with one exception so far.** Sixteen crates were built by sixteen agents, and the interfaces between layers held. The exception was the plan: the compiler validated a dozen things from target files that the plan had no place for, above all the whole of the sign-in file. One follow-up pass fixed it before any crate above was written against the gap.
- **Starting a layer before the layer below has finished costs little.** An agent told that a dependency is being finished starts with its outside crates and its pure logic, and meets the dependency's real interface when it lands. No agent wrote a stand-in.
- **Every large outside crate that the design named was refused on evidence,** and its replacement was small: the CEL interpreter, Cedar, a JSON Schema validator, the spreadsheet and csv readers, a DevTools client. What was kept from outside is what should not be written twice: a cipher, a key store, SQLite, a TLS stack, the MCP SDK, a YAML event parser, Unicode normalization, decimals.
- **The closed licence list earned its keep.** It refused Cedar's build tree, two DevTools crates, and a WebDriver client, each for a real reason that a looser list would have let through.
- **A shared tree needs shared numbers.** Decision records and store migrations are numbered, and agents working at once need their numbers handed out before they start.

## Changed in the design

- Decisions 0067 to 0093, 0121, and 0122. Decision 0033 is superseded by 0082.
- Three licences joined the closed list, each on purpose and each for a named crate: `Zlib` (foldhash, in `rusqlite`'s tree), `ISC` and `CDLA-Permissive-2.0` (ring and the bundled root certificates, in the TLS stack that `a2m-auth` uses behind a feature that is off by default).
- Five crates were added for the command, data, file, stream, and device packs, with their rows in the layering table.
- Toolsets are not target files; they live in the store. An environment is part of the target file.
- A viewport is a field of a web surface, not of the kernel's environment.

## Noticed, not acted on

- **Windows and macOS had run nothing when this was written.** Agents checked that their gated code compiles and lints for both, where a crate has no C in its tree. Pull request 11 is the first run.
- **TLS is off.** With the TLS feature on, every HTTP client in a build must have a cryptography provider installed in its process, or it stops when it is made. Until one change installs it everywhere, an `https` identity service is refused by name, and only loopback endpoints are used.
- **Named time zones are not built;** a target's zone is `UTC` or a fixed offset.
- **The ledger grants a record lock to whoever asks first,** so a run woken by the scheduler can lose the lock to a run that never waited.
- `a2m-telemetry` and `a2m-policy` each define a vocabulary that `a2m-core` now also has, and three crates still carry their own shape checks for names that the core's macro can make.

## Decided without the owner having read it

Every decision from 0067 on. The ones with the widest reach: the order of the build (0067); the policy engine is our own, not Cedar (0082); expressions are our own subset of CEL with no floating-point numbers (0072); the browser engine is our own DevTools client (0070); the MCP service uses the official SDK (0071).

## For the owner

Nothing new. The three items from before still stand: the pending publisher on PyPI and a manual run of the release workflow; one first publish each on crates.io and npm; private vulnerability reporting.

## Next

*To be completed when the session ends.*
