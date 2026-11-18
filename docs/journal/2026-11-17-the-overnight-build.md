# 2026-11-17: the overnight build

This entry was written while the build was under way and is brought up to date as each wave lands. Its two closing sections say what was true when pull request 13 merged, on 2026-11-18. The work that followed is in [the wider reach](2026-11-18-the-wider-reach.md).

## State at the end

Pull request 13 merged into `main` on 2026-11-18 (merge commit 631d3ea) at head 1310c1f, with every check green on Windows, macOS, and Linux and the two publish jobs skipped as intended. The kernel, nine packs, the local engine, the two front doors, the clients, and the flow that builds a tool are on `main`, and the first-hour flow and the engine loop run against the test stands. Nothing has been tried against a real system, and nothing is published. On Linux the validated commit before the last fixes passed 1,742 tests with none failing (the lead's count at the time; this entry's author did not rerun it).

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

**Wave 4: the contract.** `a2m-provider`: the manifest and its checks, the registry as an immutable snapshot with a digest, the driver interface behind a handle that refuses a committing step without the ledger's intent token, discoverers that cannot write by the types, checked paths, and the supervisor for programs run to completion and programs that speak over pipes (0088 to 0090). A third small pass put a surface's declared programs in the plan and let a schema check run-time values (0124).

Pull request 11 carries everything above. It merged green on Windows, macOS, and Linux, after four runs: the first found the advisory, the second and third the three defects described under Learned.

**Wave 5: the loop, the registry, and the first packs.**

- `a2m-engine`: build-order steps 1 to 7 proven against the test application through a real driver and a real store behind `Control`: a read through every layer, the signed trace, the plan's hash checked at guard, `needs_login`, a verified write, comparison after normalization, and resume without a duplicate. Steps 8 to 10 (identity strategies, dependent records, pipelines, sources, the fault rows of section 224, the sandbox) are written and have no test yet.
- `a2m-actions`: all 91 catalog actions as data, 26 served; the settings loader; `Kernel::prepare` and `Kernel::open`; the in-process `Control` and runner loop; the dispatcher; the person surface and the approval desk (0094, 0095). A tool call reads end to end, and a write rests for approval, is approved at the desk, and is written once.
- Seven packs, each proven against its test stand through the driver handle with real intent tokens: HTTP (0096, 0097), command (0098, 0099), data (0100, 0101), file (0102, 0103), stream (0104, 0105), device, and MCP. None had yet run through the engine loop when this was written.
- `a2m-pack-web-browser`: the browser engine on our own DevTools client, with 13 tests that drive a real browser against the test application (0110, 0111).

**Wave 6: the web pack, the front doors, and the product.**

- `a2m-pack-web-ui`: the web pack's drivers on the browser engine: screens declared in files, values read from the page, a form filled and saved with the write verified, a write in doubt when the browser is lost after the commit point, and a password never read (0112, 0113). `a2m-pack-web-probe`: its discoverers (0114).
- `a2m-mcp`: the MCP service, serving two protocol revisions without a session, and the stdio proxy (0115). It first stood on the official SDK; the advisory check then refused a crate in the SDK's tree, so the inner layer is now the crate's own, about a thousand lines ([decision 0130](../decisions/0130-the-mcp-protocol-layer-is-our-own-because-the-sdk-brings-an-unmaintained-crate.md)).
- `a2m-api`: every served action at one route, five resource aliases, and a generated OpenAPI document (0116). `a2m-local`: the engine lock, the local channel with its peer check, the connector, and the loopback approval surface (0117).
- `anything2mcp`: the product crate joins the kernel and every pack in one process behind the generated command line. Its tests prove the first four milestones on Linux against the test stands: a read; a read behind a sign-in; one verified write that rests, is approved, and is written once; and one read through every pack, the browser included (0119).

Pull request 12 carries waves 5 and 6. It merged green on the three operating systems after six runs.

**Wave 7: the safety audits, the doors joined, the first clients, and pull request 13.** Pull request 13 carries everything between pull request 12 and its merge: decisions 0131 to 0175 and 0176 to 0204, in large part. The parts that matter most:

- **Read-only audits by readers who had not built the crate.** Each kernel crate and pack was read against the documents and the ten rules. The audits found about six critical and twenty-five serious gaps in crates whose own tests all passed, and the fixes are the "after their safety audit" records, 0138 to 0153. A later round of audits (the fourth, on the kernel) found three serious faults that were fixed in 0200 and 0201.
- **The doors joined** (0131): `serve`, the thin client, and the stdio bridge, with two unrelated MCP client libraries calling through the bridge. A decline ends a run, a resume is never lost, a lost answer is a write in doubt at the doors and in the clients (0132, 0159).
- **The presence prompt on each system** (0133, 0142, 0146), the wheel and the npm packages (0135, 0186), the clients generated from the served document (0156), a client that starts the engine on demand (0165), and HTTPS in the product (0157, 0166).
- **The stream pack speaks a real broker's wire protocol through a client of our own**, and the data pack reaches a server database (0167, 0168). Both ran in CI against a real server for the first time on this pull request; neither had been run against one before.
- **The tool-building flow:** the scout, a report that is hashed data, a ledger that ends a record on an acknowledgement (0170 to 0172), what each pack drafts (0179), offline tests that replay fixtures and gate the publish (0177, 0180), and a first hour that runs with no hand step (0189, 0199).
- **The desktop pack, reading Linux applications through a look-only back end** (0198), and the first release scoped to one machine (0169).
- **Fixes found by CI's first runs.** Two rounds on Windows and macOS: a path's plain form (a real Windows bug: trust roots never matched), a named-pipe client that waits as long as its caller will, a named stop for a socket path too long for macOS, an access-list test that compared text where it should compare identities, and the npm package check (0204). Then one race in a test, in two places: a late request for the page's icon made the stand's request count differ by one between two looks, and the presence test asked the authorization service before it had dropped a stopped agent. Both were fixed in tests, with no check loosened.

## Learned

- **The designs were ready to be built in parallel, with one exception so far.** Sixteen crates were built by sixteen agents, and the interfaces between layers held. The exception was the plan: the compiler validated a dozen things from target files that the plan had no place for, above all the whole of the sign-in file. One follow-up pass fixed it before any crate above was written against the gap.
- **Starting a layer before the layer below has finished costs little.** An agent told that a dependency is being finished starts with its outside crates and its pure logic, and meets the dependency's real interface when it lands. No agent wrote a stand-in.
- **Every large outside crate that the design named was refused on evidence,** and its replacement was small: the CEL interpreter, Cedar, a JSON Schema validator, the spreadsheet and csv readers, a DevTools client. What was kept from outside is what should not be written twice: a cipher, a key store, SQLite, a TLS stack, the MCP SDK, a YAML event parser, Unicode normalization, decimals.
- **The closed licence list earned its keep.** It refused Cedar's build tree, two DevTools crates, and a WebDriver client, each for a real reason that a looser list would have let through.
- **The three operating systems found four things in their first hour, and each was real.** The advisory check refused three unmaintained crates under the Linux key store, so that back end was removed and the check left alone ([decision 0123](../decisions/0123-the-linux-secret-service-back-end-is-left-out-until-its-store-crate-is-maintained.md)). On macOS, a signal to a process group whose only member is its exited leader answers "not permitted", which the supervisor read as a group it could not end. On Windows, a program that the supervisor ended looked like one that exited by itself with status 1, so a write cut off by shutdown would have read as an ordinary failure and not as a write in doubt. And a keeper test read what the keeper's task does after its last write without waiting for it, which the scheduling on Windows exposed; tracing it found two real inaccuracies in how the keeper reported being idle. None of the four could have been found on Linux.
- **Every pack asked the provider contract for the same few things.** A place in a step's outcome for how the step was done, so the trace's signature can cover it; untrusted text from the system in a failure; the values kept for reconcile handed back to a step on resume; a clock for a discoverer; a session's lease at the moment of a request. Six agents who never saw each other's work each listed them. That agreement is the measurement hypothesis H-26 asked for: the kernel is neutral in shape, and its contract was a little too thin. The command pack forced one change to the plan (decision 0124); the rest wait for one pass after the end-to-end proofs exist.
- **The second pull request found five more things on the other two operating systems.** On Windows the engine's ends of a child's two named pipes were made and never connected, so no browser could answer; four browsers started cold at once took longer than the launch deadline, which read as "not a browser" when it was a slow start ([decision 0134](../decisions/0134-one-browser-starts-at-a-time-and-a-silent-start-is-slow-not-foreign.md)); and two enum variants made only on Unix failed the lint. On macOS a refusal sent before the request's body was read reset the connection under the caller, who then saw no answer at all; the REST door now reads and discards the body first.
- **Joining the crates found what no crate's own tests could.** The product crate's agent found that a decline did not end a run, that a resume could answer `needs_approval` for an approved run, and that reading a run asked the person again. Each crate's tests passed. The defects were between them.
- **A disk that fills destroys files quietly.** A dozen agents building into one target folder filled the disk; files being written at that moment were left empty, and two agents had to rewrite theirs. Builds now carry no debug information and no incremental state, which made the folder nine times smaller.
- **A reader who did not build the code finds what its tests do not.** The audits of waves 5 to 7 are the evidence: the faults were between a crate's promises and its callers, in failure classes that decide lock release, and in paths a test never walked. Audit after each crate, with a fresh reader, is now a standing step.
- **Real servers found nothing in the clients, and Windows found real faults in the plumbing.** The two network clients met a real server on their first CI run and passed. The first Windows and macOS runs of the supervisor, the pipes, and the path code each failed for a real reason, in a different place each time.
- **A shared tree needs shared numbers.** Decision records and store migrations are numbered, and agents working at once need their numbers handed out before they start.

## Changed in the design

- Decisions 0067 to 0204, in large part. Decision 0033 is superseded by 0082.
- Three licences joined the closed list, each on purpose and each for a named crate: `Zlib` (foldhash, in `rusqlite`'s tree), `ISC` and `CDLA-Permissive-2.0` (ring and the bundled root certificates, in the TLS stack that `a2m-auth` uses behind a feature that is off by default).
- Five crates were added for the command, data, file, stream, and device packs, with their rows in the layering table.
- Toolsets are not target files; they live in the store. An environment is part of the target file.
- A viewport is a field of a web surface, not of the kernel's environment.

## Noticed, not acted on

- **The session's usage limit was reached once,** with the engine, the action registry, and eight packs half-built. All twelve agents stopped at once. Their files were on disk and each was resumed from where it stopped when the owner reset the limit.
- **TLS is off.** With the TLS feature on, every HTTP client in a build must have a cryptography provider installed in its process, or it stops when it is made. Until one change installs it everywhere, an `https` identity service is refused by name, and only loopback endpoints are used.
- **Named time zones are not built;** a target's zone is `UTC` or a fixed offset.
- **The ledger grants a record lock to whoever asks first,** so a run woken by the scheduler can lose the lock to a run that never waited.
- `a2m-telemetry` and `a2m-policy` each define a vocabulary that `a2m-core` now also has, and three crates still carry their own shape checks for names that the core's macro can make.

## Decided without the owner having read it

Every decision from 0067 on. The ones with the widest reach: the order of the build (0067); the policy engine is our own, not Cedar (0082); expressions are our own subset of CEL with no floating-point numbers (0072); the browser engine is our own DevTools client (0070); the MCP service uses the official SDK (0071).

## For the owner

Nothing new. The three items from before still stand: the pending publisher on PyPI and a manual run of the release workflow; one first publish each on crates.io and npm; private vulnerability reporting.

## Next

1. Validate the whole tree that came after the merge and take it through CI on three systems: the work of [the wider reach](2026-11-18-the-wider-reach.md) has not been through CI.
2. Fix what the audits of that work find, then the order the reviews ranked, as that entry lists.
3. Still open from this entry: TLS is on in a default build but each client must install its cryptography provider; named time zones are not built; the ledger grants a record lock to whoever asks first.
