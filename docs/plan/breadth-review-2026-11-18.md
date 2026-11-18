# Breadth review, 2026-11-18

One design review round, by a new lens, [lens 16](design-review-lenses.md), breadth: if the product is to be truly "anything to MCP", along which dimensions of "anything" is it still narrow? The question is the owner's, put by the lead agent; the wording here is this round's own ([decision 0231](../decisions/0231-the-owners-direction-is-written-in-exact-words-of-the-agents-own.md)). The lead's reading of the question, which this round took as its brief: not settings and not more of the same, but whole dimensions the design does not yet have.

Two dimensions were left out on purpose, because other work holds them: the kinds of system a machine can reach (the [coverage map](coverage-map.md) and the [coverage review](coverage-review-2026-11-18.md), with records 0211 to 0215), and the network the machine is on ([network awareness](network-awareness.md), [decision 0222](../decisions/0222-the-product-is-aware-of-the-network-its-machine-is-on.md)). Where a finding here touches either, it says so and goes no further.

The round wrote documents only: this review, decision records [0232](../decisions/0232-whether-a-credential-the-machine-holds-for-the-person-may-be-used-through-its-holder.md) and [0233](../decisions/0233-whether-being-told-by-another-machine-is-serving-it.md) (owner's questions, Proposed), [0234](../decisions/0234-a-watch-is-a-read-repeated-on-a-schedule-a-person-confirmed-and-a-change-is-reported-never-acted-on.md) (a design record, Proposed), and lens 16. Every "missing" thing below carries its kind exactly as [SCOPE.md](../../SCOPE.md) uses the word: **Rule** (a rule or an owner decision stands in the way; it is a question for the owner, never a recommendation to break it), **Choice** (a design decision in force; any agent may change it with evidence), or **Not yet** (nothing stands against it). Sizes are S, M, L. "Kernel" and "pack" say where the work falls.

## The evidence read

Code, not only documents, wherever a claim is about what is built:

- `crates/a2m-mcp/src/protocol.rs` in full, and `tools.rs`, `http.rs`, `proxy.rs` by search: the methods served, what a tool list carries, progress and cancellation.
- `crates/a2m-actions/src/catalog.rs` and `handlers/mod.rs`: the 92 actions registered and which have handlers (about 49).
- `crates/a2m-api/src/lib.rs` and the crate design of `a2m-api`: the REST face, a run's events as server-sent events, webhooks (designed, not built), the toolset export (designed, not built).
- `crates/a2m-auth/src/providers/` (all four files): the providers built. `crates/a2m-pack-web-ui/src/lib.rs`: the web pack's sign-in providers, not built.
- `crates/a2m-sched/src/lib.rs`, `scheduler.rs`, `types.rs` by search: deadlines kept in the store's clock and a sweep.
- `crates/a2m-files/src/types.rs`: the built-in field types and the zones a time may be in. `crates/a2m-sources/src/csv.rs`: the encodings read. `crates/a2m-expr/src/measure.rs`: the kinds of unit.
- `crates/a2m-policy/src/rules.rs` and `set.rs` by search, and the product crate and `a2m-actions` for where pre-approval rules are loaded.
- `crates/a2m-engine/src/run.rs` by search: how a pipeline step finds its operation.
- `bindings/README.md`: what the Python and Node clients give.
- Spec sections 201, 205 (pipelines), 207 (failure classes), 208 (states), 210, 212 (approvals), 215 (sign-in), 221, 222 (engine lifetime), 401 (watching for change), 402, 503, 604 (triggers), 704 (not designed); the action catalog; the file-format reference by search; the glossary by search; [research note 18](../research/18-mcp-server-base.md) for what the protocol's two revisions define; decisions 0205, 0208, 0211, 0230, 0231.

What the protocol's revisions contain is taken from research note 18, which read the specification's repository on 2026-11-17. This round did not read the specification itself.

## 1. Kinds of interaction

**What exists.** The verbs `open`, `read`, `list`, `search`, `create`, `update`, `upsert`, `delete`, `upload`, `download`, `submit`, `call`, and in the stream pack `await`, `publish`, `take`. No built pack registers `upload`; only the MCP pack registers `download` ([technical reference](../reference/technical-reference.md#verbs-by-mode)). A run has states for waiting on a person (`needs_input`, `needs_approval`, `needs_login`). A long call returns a `run_id` at once, and `run.get`, `run.resume`, `run.cancel` are built. A caller that asks for progress gets it, read from the run's events. Over REST a run's events are a page or a stream that resumes from the last event. Pipelines run operations over a source with a checkpoint per record and resume after a lost answer (roadmap 3.9). A stream `await` is a bounded wait for one matching message. A write that cannot be read back is `irreversible` and needs a person every run.

**What is missing.**

| Missing | Kind | Size | Kernel or pack |
| --- | --- | --- | --- |
| **Being told.** Every tool is asked; none tells. A system changes on its own (a row appears, a status moves, a file is replaced) and an agent learns of it only by asking again, between turns it does not have. Section 704 leaves "triggers, and runs started by an event" to Phase 6 or later | Not yet | M | Kernel: a watch is a read repeated on a schedule, compared by digest. No pack changes. Designed in [decision 0234](../decisions/0234-a-watch-is-a-read-repeated-on-a-schedule-a-person-confirmed-and-a-change-is-reported-never-acted-on.md) |
| **A value watched until a condition holds** for any read, not only a stream message | Not yet | S, once watches exist | Kernel: a watch's condition, an expression over the newer result |
| **Being told by an event a system sends.** An outside service's webhook cannot arrive: nothing listens for another machine | Rule: owner decision [0208](../decisions/0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md) | — | Question in [decision 0233](../decisions/0233-whether-being-told-by-another-machine-is-serving-it.md). Inside the rule: polling by a watch, or reading a relay or broker the person already uses through an outbound connection |
| **Things that happen at a time**: a read every morning, a write approved now and carried out at nine tomorrow | Not yet (section 704) | M | Kernel: the scheduler already keeps deadlines in the store's clock; a scheduled write re-reads its diff at the moment and stops as `stale` if it differs from what was approved |
| **Long work in the protocol's own form.** Runs are tasks in all but name; the Tasks extension is designed in the crate design of `a2m-mcp` and not built | Not yet | S to M | The MCP door only |
| **Many records with partial failure.** A pipeline stops at the first record the system refuses (`rejected` stops the run, section 207), and resumes from there. There is no "go on with the others and report each one" | Choice (section 207, safe first) | S to M | Kernel: a pipeline step may declare that a `rejected` record is reported and passed, never a `possible_commit` or a `mismatch` |
| **Work across two systems that must both succeed.** A pipeline names operations of its own target's plan only (`a2m-engine` `run.rs` finds a step's operation by its index in one plan), so two systems are joined today only by declaring both as surfaces of one target. Section 106 says systems of systems are "a pipeline over several targets": that is not true today. Compensation (undo the first when the second fails) is not designed; `run.undo` is designed for Phase 9 and not built; the device pack chooses a declared safe state a person invokes over any automatic undo | Not yet for several targets; Choice for no automatic compensation (rule 2 favours it: a compensating write is a write the person did not see) | M; L for compensation | Kernel: a pipeline that names another target's published operation by name and version; compensation, if ever, as declared reverse operations shown in the same diff and approved with it |
| **Large or binary things in and out.** A tool cannot hand back a file (no resource links, no `resources/read`, no download verb outside the MCP pack) or take one in except a pipeline's source, a path inside an allowed root | Not yet | S to M kernel, S each pack | Kernel: an output folder of the project and a result field of type `file` that names a path and a digest; resource links for kept blobs. Packs: `upload` and `download` in the HTTP and web packs; the command pack's `collect` step (designed) |
| **Interactive sessions with state**: a wizard, a prompt-and-answer program, a terminal screen, a debugger | Not yet for the first three (coverage map: left out, L); Rule for a debugger (attaching to a running program, rule 5) | L | A pack: an interface mode over a pseudo-terminal, every prompt declared, any undeclared prompt a stop (rule 7) |
| **Asking the person a question in the middle of a run.** `needs_input` exists as a state; nothing in the MCP door carries the question | Not yet | S | The MCP door: the stateless revision's round trip for input (`input_required`) carries a caller's input. An approval never travels that way (see section 2) |

## 2. What the product offers through MCP, beyond tools

**Served today** (read in `a2m-mcp/src/protocol.rs`): `initialize` and `ping` of revision 2025-11-25; `server/discover` of revision 2026-07-28, with server instructions; `tools/list` as one page, each tool with its name, title, description, input schema, and annotations, and no output schema (a test in `tests/checklist.rs` holds the list to that; `tool.describe` gives the schema); `tools/call` with structured content and one text item; progress notifications to a caller that sent a token; cancellation, which stops the waiting and leaves the run going. The capability declared is `tools` alone. Everything else is an unknown method.

| Feature | Today | What the rules say | What it would add |
| --- | --- | --- | --- |
| Tools, structured output, annotations | Served | — | — |
| Progress | Served, for calls that start or continue a run | Notes are the server's own sentences; no system text | — |
| Cancellation | Served: the waiting stops; `a2m_run_cancel` stops the run | — | — |
| Server description for discovery | `server/discover` with instructions, served | — | — |
| Tool search | Served under another name: `a2m_tool_search` and `a2m_tool_call` | — | — |
| Resources and resource links | Not served. Designed: traces, diffs, snapshots, and files as resources linked from results (section 221) | Allowed | Files out; a watch's state; a run's diff as a link |
| Subscriptions (`subscriptions/listen`, which in 2026-07-28 replaces `resources/subscribe` and carries list-changed notices, by research note 18) | Not served. The crate design says found tools cannot be added to a client's list "until a client can be told of a changed list without a session"; note 18's reading of 2026-07-28 is that `subscriptions/listen` is that way. The reason given is out of date | Allowed | Told when a watch sees a change, and when a publish changes the tool list |
| Long-running tasks (extension `io.modelcontextprotocol/tasks`) | Not served; designed as runs | Allowed | Long work for clients that manage tasks themselves, instead of calling `a2m_run_get` |
| Elicitation by link (URL mode) | Not served; the fallback, a link in the result, is built | Allowed: following the link is never an approval by itself | The sign-in and approval pages opened by the client |
| Round trips for input (`input_required`, 2026-07-28) and form elicitation (2025-11-25) | Not served; not designed | **Choice**, resting on rule 2: what passes through the client may be the agent's words, so a person's approval, a credential, or a test record never travels this way. A caller's input at a declared decision point (`needs_input`) may, because it is no more than the caller's input | Asking mid-run |
| Prompts | Not served; designed (roadmap 5.1, generated from the same sources as `docs.get`) | Allowed: fixed text, no system text | Guided flows for clients that show prompts |
| Completions | Not served; not designed | Allowed if every value comes from declared data (a choice list, a target's name, a published tool's name), never from a live read at completion time | Help choosing arguments, S |
| Sampling | Not served. Deprecated in 2026-07-28 (note 18) | Refused in a run by **rule 1**. At build time it is a **choice**: principle 4 brings the developer's own agent and the scout is a fixed pipeline, and the building agent is already a model | Little; not wanted |
| Logging | Not served. Deprecated in 2026-07-28 | — | Nothing the run events and traces do not give |
| Roots | Not served. Deprecated in 2026-07-28. Served under another name: the project folder the mount states, never set by a message | — | — |

**The other doors.** REST serves every action generated from the registry, its OpenAPI document, and a run's events as a page or a resumable stream; webhooks are designed and not built. The Python and Node clients call any action, one generated method per action, and start the engine when none runs. A program that does not speak MCP can therefore call a wrapped tool today, through `tool.call` by name over REST, a client, or the command line. What it cannot get is a description of each wrapped tool in its own format: the export of a toolset as an OpenAPI document, one operation per tool, is designed in the crate design of `a2m-api` and not built (Not yet, S), and nothing gives a typed method per wrapped tool in the clients (Not yet, S).

## 3. How a tool comes to exist

**What exists.** The scout starts from an address a person names and drafts reads by rule from what a system states: an OpenAPI description (built in the HTTP pack; the `spec.import` action is not built), an upstream MCP server's tool list, a database's catalog, a file's structure, a device's published map, a web page's tables and its own GET requests, and a program's help (command definitions only, no operation, because help does not say which commands read). A script the developer keeps is a fixed pair with a declared interpreter ([decision 0216](../decisions/0216-the-command-pack-on-three-systems-the-administrators-group-and-a-script-the-target-keeps.md)).

| Missing | Kind | Size | Kernel or pack |
| --- | --- | --- | --- |
| **A person doing the task once while the product watches.** `capture.start`, `capture.stop`, `endpoints.infer`, and `learn` are in the catalog, with no handler. Designed for the web pack only: a watching tab in a visible window that the person drives, exchanges attributed to the step of a scenario, dependencies found by digests of values, cookies and authorization removed by name (section 301). Needs: the watching tab, the scenario format (not designed, section 704), endpoint inference, and the person surface to start and stop it. Nothing is designed for watching a person use a desktop application (accessibility events) or a terminal | Not yet | L | The web pack, then the desktop pack; the kernel's scenario format |
| **Traffic the person already recorded** (a browser's recording of requests) | Not yet | S to M, after H-41 | A discoverer in the HTTP pack. Such a file holds cookies and tokens: scrubbed by name before anything is kept (rule 4) |
| **Other descriptions**: RAML, API Blueprint, collections, OpenRPC, AsyncAPI, WSDL, gRPC, PowerShell's own command metadata, manual pages and completions | Not yet | S each after H-41; gRPC M to L | Covered by the coverage map; not repeated |
| **Another automation tool's recorded flows** (a recorded browser test, a desktop automation script) | Not yet | M per format | A discoverer per format. A recorded flow is a person's description of a write sequence: it can draft a write's mechanics, never its meaning |
| **Writes drafted from what a system states.** Today every write is written by hand ([section 402](../spec/4-building-tools/402-the-scout-and-who-does-what.md)). Some systems state everything mechanical about a write: an OpenAPI `PUT` or `DELETE` on a path whose parameter is the key and whose `GET` on the same path is the read-back; a GraphQL mutation that returns the object; a table with a primary key (a keyed update and the read by key); a register the device lists as writable and readable; an MCP tool the server marks idempotent and not destructive. These could be drafted as the endpoint, the body's schema, the key, and the read-back read, with the judgment left as open questions so that the draft does not compile until a person settles them: the effect class beyond `write`, any hazard, whether it is `irreversible`, the test record, names, and units. The open-question mechanism exists ([section 218](../spec/2-kernel/218-discovery-and-findings.md)) | Choice (section 402; decision 0179). The evidence favours changing it for stated contracts only: nothing would be guessed, and nothing would compile unsettled | M kernel drafter, S each pack | Kernel: a drafting rule for a described write; packs: the lowering |
| What must stay a person's judgment | — | — | Meaning, the test record, the effect and hazard, and whether a write may be repeated where the system does not state it. Unchanged |

## 4. Who and what a tool acts as

**Built** (`a2m-auth/src/providers`): `api_key`, `bearer_static`, `http_basic`, `http_digest`, `windows_integrated` (which holds a password and hands it to the driver for Negotiate or NTLM), `client_certificate`, `oauth_client_credentials`, `oauth_authorization_code` with a link the person opens, and a one-time code computed from a stored secret for a dedicated account. A person types a credential or a code on the engine's own page ([decision 0205](../decisions/0205-a-sign-in-an-agent-asks-for-is-carried-out-by-its-person-on-the-engines-page.md)). The upstream MCP server's own authorization is in the MCP pack. **Not built:** `form_login`, `sso_oidc`, and `sso_saml` in the web pack (with the sign-in window), `os_broker`, and `obo_token`.

| Kind of identity | State | Kind | Size |
| --- | --- | --- | --- |
| **A web application's own sign-in page**, with or without a second factor. Not only "with a second factor", as SCOPE.md says: no sign-in through a page is built, so a web application that signs in by a form or by single sign-on through pages cannot be used today | Not yet | M to L | The web pack |
| A second factor answered by the person (a code on the engine's page, a push on their phone): inside rule 5, as the person's own act. Answering it without the person: refused | Not yet within `form_login`; Rule 5 for the bypass | — | — |
| **The operating system's sign-in broker** (single sign-on on a managed machine) | Not yet (Phase 7, per system; section 215 designs it as allowed) | M each | `a2m-auth`, per system |
| **The operating system's integrated sign-in with the current logon** (Negotiate through the system's own security interface, with no password held) | Not designed. Whether the logon ticket is the system's own or "another program's store" is the owner's reading of rule 5 | Rule 5: question 1 of [decision 0232](../decisions/0232-whether-a-credential-the-machine-holds-for-the-person-may-be-used-through-its-holder.md) | M |
| **A key that signs only with the person present**: a smart card, a passkey in the engine's own browser through the system's prompt | Not designed | Rule 5: question 2 of 0232 | M each |
| **Keys held by a signing agent another program runs** (SSH agent, GPG agent) | Not designed | Rule 5: question 3 of 0232. Inside the rule: SSH with a key the engine's own vault holds, for which no transport is designed (Not yet, M) | — |
| **Another program's stored credentials** (a cloud command-line tool's credential file or credential process, a version-control credential helper, a password manager) | Refused | Rule 5: question 4 of 0232 | — |
| **A cloud provider's request signing** (each request signed over its method, path, body digest, and time) | Not yet. The provider places a credential once per lease (section 215: a header, a cookie, a TLS identity, or a challenge credential); signing each request is a new shape of the auth-provider interface, a public contract | Not yet | M: `a2m-auth` and the HTTP pack, with a decision record |
| A dedicated account in place of the person | Not yet: "the account is the caller's person; a dedicated account's name waits for toolsets" (decision 0205) | Not yet | S to M |
| Acting as another person | Never: "a user's session is theirs alone" | Choice, a convention in AGENTS.md resting on rule 4 | — |

## 5. Where it runs and whom it serves

The first release is one person's machine ([decisions 0169](../decisions/0169-the-first-release-is-for-one-machine.md) and 0208); this section does not reopen that.

| Case | What is true | Missing, and kind |
| --- | --- | --- |
| **Other machines the person owns** | Reached through what they already serve: an HTTP interface, an MCP server over HTTP, a database, a broker. A remote shell is refused (the command pack offers no shell). The engine on the other machine cannot be reached, because it listens for nobody but its own machine | SSH with a fixed command: Not yet, M, and its key is question 3 of 0232. The engine of the person's second machine: Rule (0208), question 2 of [decision 0233](../decisions/0233-whether-being-told-by-another-machine-is-serving-it.md). A runner on another machine: Rule (0169, after the first release) |
| **A machine with no person at it** (a build server, a scheduled job) | The in-process mode runs the engine inside one command for CI ([section 222](../spec/2-kernel/222-front-doors-and-the-local-engine.md), decision 0165). Reads run. A write waits at `needs_approval` with nobody to approve: over SSH, in a container, or on a headless machine nothing can approve ([section 503](../spec/5-security/503-approval-and-sign-in-surfaces.md)). The policy engine evaluates pre-approval rules, but nothing gives the running product one: no file and no action loads them (`PreApproval` is used only inside `a2m-policy`; `policy.publish` has no handler). On Linux the vault takes a passphrase (decision 0123), which a client hands the engine on a pipe, so an unattended engine there holds no stored session. A solo engine stops when idle, so nothing happens between calls | **The statement is missing**: what can and cannot happen with no person is said nowhere in one place (Not yet, S, documents). Pre-approval rules a person can give the running product: Not yet, S to M. An engine that stays running while something is due: Not yet, S (part of 0234) |
| **Several users of one machine** | Built: each user's own engine and channel; a different user is refused (roadmap 1.4) | Nothing |
| **A machine inside a boundary** (a container, the Linux subsystem on Windows) | The engine runs there; nothing there can approve, by design (section 503); the desktop pack cannot reach the host's windows from inside | Reaching the host's applications from inside the subsystem: Not yet, not designed; probably a non-goal |
| **A phone** | As a client: refused, it is another machine (0208). As a target: a device by cable is cataloged (coverage map, L). As the place a person approves from: see section 11 below | — |

## 6. The shapes of data

| Missing | Kind | Size | Where |
| --- | --- | --- | --- |
| **Named time zones.** A time's zone is `UTC` or a fixed offset such as `+02:00` (`a2m-files/src/types.rs`, `utc_offset_minutes`; the file-format reference's `environment.timezone`). A zone with daylight saving cannot be stated, so a local time in most of the world's business systems is either refused or wrong for half the year. With named zones, a wall time that occurs twice or never on a change of the clock must stop by name (rule 7) | Not yet | S to M, with a time-zone database dependency to verify | Kernel: `a2m-expr` and `a2m-files` |
| **Files as results and as inputs** | Not yet | See section 1 | Kernel and packs |
| **Very large results.** The data pack fetches at most `max_rows` and says that more exist; the HTTP pack designs a pager per style. Section 221 promises results "paginated by cursor, size-capped with a note"; this round found no cursor for a target's results in `a2m-actions` (only for tool lists and the MCP client) and did not verify the cap. No result is streamed | Not yet | M | Kernel: a cursor over a kept result; the packs' pagers |
| **Request bodies that are not JSON or a form; binary protocols other than the device pack's frames** | Not yet | Covered by the coverage map | Packs |
| **Character sets.** Source files are read as UTF-8 only, and anything else is refused by name (`a2m-sources/src/csv.rs`). Honest; a file from an older system in another encoding cannot be read | Not yet | S | Kernel: a declared encoding in a source |
| **Units.** The expression library knows lengths and masses. Temperatures (a conversion with an offset), volumes, and durations are not there | Not yet | S each | Kernel: `a2m-expr` |
| **Money across currencies.** A field fixes one currency; another currency's code stops. Conversion needs a rate, which is a business fact and not a unit | Choice (section 210): rightly a business step, an operation, not a field type | — | — |
| **Structured refusals from systems.** A refusal is `rejected` with the system's message as untrusted text and the record key. An agent cannot branch on the system's own reason (a duplicate, out of stock) without reading that text | Not yet | S | Kernel: an endpoint or command may declare refusals by status and a path to the system's code, each a checked name in the result |
| Locales | A decimal mark per field and one locale per target environment. Enough for one system; see section 12 for a person's language | — | — |

## 7. The operating system itself as a system

The coverage map has rows for folders, processes, services, scheduled tasks, settings stores, installed software, network settings, notifications, windows, the clipboard, printing, system management, and event logs. Nearly all are "through the command pack": they fall out of declaring a system program as a command, one target at a time. **"The machine itself" is not a first-class target.** There is no surface kind for it, the scout does not start from it (it starts from an address), and nothing is drafted for it.

| Missing | Kind | Size | Where |
| --- | --- | --- | --- |
| The machine as a target the scout starts from, with reads drafted for its own facilities (processes, services, folders, settings, installed software) | Not yet | M | A pack that knows the systems' own facilities as code. [Network awareness](network-awareness.md) already proposes "rung 0" built-in read tools over the engine's own machine; the two should be one design. Rule 9 bears on how: a pack that knows a kind of system as code is inside it, as every pack is; a shipped set of target files for the systems' own programs would come close to the catalog rule 9 forbids. This round reads it as inside the rule and says so, so the owner can correct it |
| Files as files: list a folder's entries, create, copy, move, rename, delete, read and set permissions | Not yet (coverage map: folders have no place) | M | The file pack: a folder mode whose records are entries, each write read back by listing |
| Services, settings, and windows as state writes read back by a query | Not yet; hypothesis H-42 | S each | Drafting guidance in the command and desktop packs |
| Writing the clipboard, sending a notification | Not yet, as `irreversible` | S | Rule 3 makes each a person's approval every run; [decision 0215](../decisions/0215-whether-a-person-may-pre-approve-an-action-that-leaves-nothing-to-read-back.md) is the owner's question |
| Reading the clipboard, reading other programs' notifications | Refused | Rules 4 and 5 (coverage map) | — |

## 8. Life after the first hour

**What exists.** A changed system is met at call time: `drift` is a failure class (a locator's fingerprint, an answer's schema, an upstream tool that changed), proven in the MCP pack (`a_changed_tool_is_drift_and_the_call_does_not_reach_the_stand`). A version is published by a person. A binding has a status `retired`. A target is a folder of plain files.

| Missing | Kind | Size | Where |
| --- | --- | --- | --- |
| **Learning that a tool drifted before calling it.** Canaries, the target's identity at every start, and the repair loop are designed (section 401) and not built (roadmap 9.1, held). Nothing tells the person between calls | Not yet | M, after watches: a canary is a watch over a surface's identity and a read's shape | Kernel |
| **Comparing and rolling back versions**: `version.diff`, `version.rollback`, `version.activate` | Not yet (no handlers) | M | Kernel |
| **Moving a target, or sharing it among the person's own machines**: the folder copies; published versions and their evidence are in the engine's store; `version.bundle` and `version.import` are designed (Phase 6) and not built. Sessions do not move, by design: the new machine signs in again | Not yet; Choice for the sessions | M | Kernel |
| **Retiring a tool or a target.** No action retires a published tool or removes a target (the catalog has no `target.remove` and no way to withdraw a published operation) | Not yet | S | Kernel; a catalog change, so a decision record |
| A newer engine with older versions: `version.migrate` | Not yet (Phase 9) | M | Kernel |

## 9. Dimensions the lead's list did not have

Four more, each tested below as sections 10 to 13. Three others were considered and dropped: undo (folded into section 1, work across two systems); the product's own accessibility for people with disabilities (real, but a quality of the approval page rather than a dimension of "anything"); and freshness of a result (a run already carries its time).

## 10. Which agent is calling, among several on one machine

A person may run a coding agent, a chat client, and a scheduled script against one engine. On one machine the person holds every role (action catalog); a client's label is kept for the audit and for fair queuing only; `policy.agent_effect_ceiling` is one setting for every caller. So a person cannot say "this agent may only read, that one may write". **Missing: a connection the person names (the configuration `client.connect` writes) that carries its own toolset and effect ceiling.** Kind: Choice ("no client is special" forbids branching on a client's make, not on a connection the person named; the evidence favours it). Size S to M; kernel (`a2m-policy`, the doors).

## 11. Where the person is

Every approval and every sign-in happens at the machine: the approval page listens on loopback, and the presence prompt is the machine's own. A person away from their desk cannot approve the write their desktop agent asks for, and a write waits until they return. Kind: Rule (0208, nothing listens for another machine) for approving from a phone; Choice ([section 503](../spec/5-security/503-approval-and-sign-in-surfaces.md)) for what counts as presence. Nothing to propose for the first release; SCOPE.md should say it.

## 12. Human languages and locales

Two halves. **The product's words**: every message, fix, progress note, approval page, and generated description is English. Not yet, M (a message catalog in the kernel, fixed sentences only, so no system text slips in). **The systems' words**: an interface is located by role and name, and a name is in the application's language, so an application shown in another language, or switched by the person, misses every locator; the target's environment fixes one locale. A screen model per locale is the honest form; nothing designs it. Not yet, M, the web and desktop packs.

## 13. Volume and speed

Records in parallel, a source read from another operation, and a binding that saves more than once in one call are not built (roadmap 3.9). A run's record limit is required for every write pipeline. Bulk interfaces that take many records in one request are not designed. Kind: Not yet, M, kernel and the HTTP and data packs. The budgets of section 225 are the measure.

## The ten additions, ranked

Ranked by how much each widens what the product can truthfully claim, and ordered so that each unlocks what follows.

| # | Addition | Kind | Size | Why here |
| --- | --- | --- | --- | --- |
| 1 | **Watches: a tool that tells** ([decision 0234](../decisions/0234-a-watch-is-a-read-repeated-on-a-schedule-a-person-confirmed-and-a-change-is-reported-never-acted-on.md)) | Not yet | M, kernel | A whole direction of interaction the product lacks, for every pack at once, with no pack change. It gives 5 its deadlines and its engine that stays up, gives 6 its canaries, and gives the MCP resources of 3 and 4 their first live use |
| 2 | **Sign-in through a system's own pages**: `form_login`, `sso_oidc`, `sso_saml`, a second factor answered by the person | Not yet | M to L, web pack | Without it the first pack serves only applications that need no sign-in or take a token. Independent of the rest |
| 3 | **Files in and out**: an output folder and `file` results; `upload` and `download` in the HTTP and web packs; resource links and `resources/read` for kept blobs | Not yet | S to M kernel, S each pack | The most common thing an agent hands over or asks for. Builds the resource face that 1 and 4 reuse |
| 4 | **The protocol's own forms for long work and for asking**: runs as tasks; `input_required` for `needs_input`; `subscriptions/listen` for list changes and watches; URL-mode elicitation for sign-in and approval links | Not yet | S to M, the MCP door only | Cheap because runs are already tasks; makes 1 push where a client listens |
| 5 | **A run at a time, and the machine with no person** stated and made to work: a scheduled run whose approved diff is re-read at the moment and stops as `stale` if it changed; pre-approval rules a person gives the running product; the engine stays up while anything is due | Not yet | M, kernel | Needs 1's deadlines and lifetime. Turns "unattended" from silence into a stated, honest case |
| 6 | **Drift told between calls, and the version actions**: canaries as watches over identity and shape; `version.diff`, `version.rollback`; retiring a tool | Not yet | M, kernel | What makes a tool built last month trustworthy this month. Needs 1 |
| 7 | **Writes drafted from stated contracts**, the judgment left as open questions | Choice (section 402) | M kernel, S each pack | Halves the hand work of a target with a description, without guessing. Independent; better after 3 for upload and download writes |
| 8 | **Named time zones** | Not yet | S to M, kernel | Small, and a newcomer's first wrong answer otherwise |
| 9 | **The machine itself as a target**: a pack for the systems' own facilities, a folder mode in the file pack, services and settings as state writes | Not yet | M | Makes "anything on your machine" literal. To be one design with network awareness's rung 0 |
| 10 | **Per-connection limits for several agents on one machine** | Choice | S to M, kernel | Small, and needed as soon as 1 and 5 let things act while the person is away |

Left out of the ten, in order of what they would add next: capture and `learn` in the web pack (L); pipelines across targets, then compensation (M, then L); the toolset exported as OpenAPI for callers that do not speak MCP (S, designed); structured refusals (S); prompts and completions (S each); interactive programs over a pseudo-terminal (L); the product's words in other languages (M).

## What a careful newcomer would be most surprised by

1. **No tool ever tells.** Every tool is asked; an agent that wants to know when a status moves must keep asking, and stops asking when its turn ends.
2. **A web application's own sign-in page is not supported**, in a product whose first pack is the web.
3. **A tool cannot hand back a file or take one in** through MCP.
4. **Times are in UTC or a fixed offset only.** "09:00 in Berlin" cannot be said.
5. **A published tool that broke is found only by calling it,** and a version cannot yet be compared, rolled back, or moved to another machine.
6. **A pipeline cannot join two targets,** though section 106 says systems of systems are reached that way.

## What is cheap because the kernel already has the idea under another name

| The addition | Already in the kernel as |
| --- | --- |
| Watches | The scheduler's deadlines in the store's clock and its sweep (`a2m-sched`); a run's events as a resumable stream (`a2m-api`); the diff's per-record classes; section 401's rule that "removed" is reported only when coverage is complete; canonical digests in `a2m-core`; expressions in `a2m-expr`; the stream pack's bounded `await` |
| Tasks | Runs: `run_id`, `run.get`, `run.cancel`, the state `needs_input` |
| Asking mid-run | `needs_input` and the keeper's `PersonAsk` |
| Roots | The project folder the mount states |
| Deferred tool lists | `a2m_tool_search` and `a2m_tool_call` |
| Files out | The ledger's blob store; the command pack's designed `collect` step; the `file` field type |
| Schedules | The scheduler's deadlines; an approval bound to a diff hash; the failure class `stale` |
| Moving a target | A target is plain files; `version.bundle` is designed |
| Callers that do not speak MCP | `tool.call` over REST and the clients; the toolset export is designed |
| Per-agent limits | The client label on every request; `policy.agent_effect_ceiling` |
| Canaries | Section 401's identity at every start and comparing versions part by part |

## Questions for the owner

| Record | The question, short | Recommendation |
| --- | --- | --- |
| [0232](../decisions/0232-whether-a-credential-the-machine-holds-for-the-person-may-be-used-through-its-holder.md) | Whether a credential the machine already holds for the person may be used through its holder without being read: (1) the system's integrated sign-in with the current logon; (2) a smart card or passkey that signs with the person present; (3) an SSH or GPG agent; (4) another program's stored credentials | Yes to 1 and 2; no to 3 for the first release, with SSH by a key the engine's own vault holds; no to 4 |
| [0233](../decisions/0233-whether-being-told-by-another-machine-is-serving-it.md) | Whether "nothing listens for another machine" (0208) covers (1) receiving an outside service's signed deliveries, and (2) the engine on the person's own second machine | Yes to both, for the first release: keep 0208 as it reads, and let watches and outbound relays carry what is told |

The machine itself as a target (section 7) is read here as inside rule 9 and is not asked. If the owner reads rule 9 as forbidding product code that knows the systems' own programs, item 9 of the ranking changes.

## Lines for SCOPE.md

For the lead to apply; this round did not edit SCOPE.md. Each is a row of its table, in the form "It does | It does not, today | Kind".

**What happens when a tool runs:**

- Add: `| Answers when it is asked: a call, its progress, and a run's events as a stream | Tell an agent when something in a system changed, or watch a value until it holds. Designed in [decision 0234](docs/decisions/0234-a-watch-is-a-read-repeated-on-a-schedule-a-person-confirmed-and-a-change-is-reported-never-acted-on.md) | Not yet |`
- Add: `| Runs a call when it is made | Run at a set time, or start a run because something happened | Not yet ([section 704](docs/spec/7-project/704-readiness.md)) |`
- Add: `| Runs a pipeline over many records of one target, with a checkpoint per record | Join operations of two targets in one pipeline; undo the first when the second fails; carry on past a record the system refused | Not yet for the first; choice for the second and third ([section 207](docs/spec/2-kernel/207-the-engine-loop.md), safe first) |`
- Add: `| Takes a file a pipeline reads, from a folder the person allows | Hand a file back as a result, or send one to a system (upload and download) | Not yet |`
- Add: `| Takes and gives times in UTC or a fixed offset | Take a named time zone with daylight saving | Not yet |`

**People and agents:**

- Add: `| Serves tools through MCP (tools, progress, cancellation, discovery), REST, the command line, and the Python and Node clients | MCP resources, subscriptions, prompts, tasks, completions, or elicitation | Not yet |`
- Add: `| | Ask the client's model to generate anything (sampling) | Rule 1 in a run; a choice at build time ([section 103](docs/spec/1-product/103-design-principles.md), principle 4) |`
- Add: `| | Let a person give one agent less than another on the same machine | Choice: on one machine the person holds every role |`
- Add: `| Shows approvals and sign-ins on the machine itself | Let a person approve from another device | Owner decision 0208 |`
- Add: `| | Speak any language but English in its own words | Not yet |`

**Secrets and sign-in:** replace the row "Sign in through a web form with a second factor (the provider is not built)" with:

- `| | Sign in through a web application's own pages, by form or by single sign-on, with or without a second factor | Not yet: the web pack's sign-in providers are not built |`
- Add: `| | Use the system's integrated sign-in with the current logon, a smart card, a passkey, or an SSH agent | Rule 5. Question open for the owner: [0232](docs/decisions/0232-whether-a-credential-the-machine-holds-for-the-person-may-be-used-through-its-holder.md) |`
- Add: `| | Use another program's stored credentials: a cloud tool's credential file, a credential helper, a password manager | Rule 5 |`
- Add: `| | Sign each request as cloud providers ask | Not yet |`

**The machine and the network:**

- Add: `| Runs inside one command for a build server or a scheduled job (the in-process mode) | Approve a write where no person is present, or keep a Linux engine's sessions without a person giving the passphrase | By design for the first; not yet for the second |`
- Add: `| Reaches another machine through what it already serves | Receive an outside service's webhook; reach the engine on the person's other machine | Owner decision 0208. Question open for the owner: [0233](docs/decisions/0233-whether-being-told-by-another-machine-is-serving-it.md) |`
- Add: `| Reaches the system's own programs one declared command at a time | Treat the machine itself as a target the scout starts from; list, move, or copy a folder's files | Not yet |`

**Building tools:**

- Change the row "Draft writes. A write is the developer's judgment and is written by hand" to keep its kind and add after it: "Where a system states a write's endpoint, key, and read-back, drafting its mechanics with the judgment left as open questions is open ([breadth review](docs/plan/breadth-review-2026-11-18.md))".
- Add: `| | Learn a tool from a person doing the task once (capture and learn) | Not yet |`
- Add: `| Publishes a version | Tell the person a tool drifted before it is called; compare, roll back, move, or retire a version | Not yet |`

## Found in other documents, for the lead

Not fixed here, because this round's edits were confined to its own files:

1. [Section 106](../spec/1-product/106-what-it-latches-onto.md) says systems of systems are reached by "a pipeline over several targets". A pipeline's step finds its operation by index in its own target's plan. A contradiction: either section 106 says "several surfaces of one target", or cross-target pipelines are designed.
2. The crate design of `a2m-mcp` says a found tool cannot be added to a client's list "until a client can be told of a changed list without a session". Research note 18 reads `subscriptions/listen` in 2026-07-28 as that way. The reason is out of date; the decision it supports may still stand.
3. SCOPE.md says the web form sign-in "with a second factor" is not built. No web sign-in provider is built.
4. Section 221 says results are "paginated by cursor, size-capped with a note when truncated". This round found no cursor over a target's result; the lead should check before SCOPE.md repeats it.
5. The action catalog has no way to retire a published tool or to remove a target.
6. A hypothesis for the register, from decision 0234: **H-47**, a per-record digest of normalized values, compared across repeated reads, reports every change a person cares about and no change across quiet checks; refuted if a read's normalized values move on every read (a time of last view, a counter), which would need a declared list of outputs a watch ignores.
7. The [documentation index](../README.md) gives 8 for the plan folder; with this review it holds 10 files.

## Honest return

The round found more missing dimensions than errors. The two that matter most are a direction of interaction (being told) and a kind of identity (a web application's own sign-in), and the second is plainly "not yet", already designed. Most of the rest is cheap because the kernel already has the idea under another name, which is the strongest sign the kernel's shape is right. The next gain is from building items 1 to 4, not from another reading.

## Not read

The specification itself (the protocol's revisions are taken from research note 18); spec sections 202 to 204, 206, 209, 211, 213, 214, 216 to 220, 223 to 226, 301 to 310 beyond what is cited; the decisions beyond those cited; the hypothesis register beyond H-40 to H-46; the crates' tests beyond the one assertion cited; the policy evaluator in full; the engine's result rendering in full (so the result cap is unverified); the web pack's browser engine; the Node client's source; and the research notes other than 18 and the headers of 27 to 29. Nothing was run but the documentation audit, and nothing on Windows or macOS was checked.
