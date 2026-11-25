# Glossary

One meaning per word. Code, files, messages, and documents use these terms and no synonyms. A word here names a thing whether or not that thing is built yet.

| Term | Meaning |
| --- | --- |
| Absence steps | The step kinds a mode lists as authoritative lookups. Only a lookup made of them may prove that a write of a record with no key did not land, and only when it answers "absent" twice, a settle time apart ([decision 0185](../decisions/0185-a-record-with-no-key-is-never-proved-absent-by-one-look.md)) |
| Acknowledged | The weakest proof a write can end on: the system answered the committing step and nothing could be read back. The record is `acted`, never verified, and only an operation declared `irreversible` may offer it ([decision 0172](../decisions/0172-the-ledger-ends-a-record-on-an-acknowledgement-and-keeps-what-the-system-said.md)) |
| Action | An authoring or administration function defined once in the action registry and exposed as a command, an MCP tool, a REST route, and an SDK method |
| Network place | Where an address is and how this machine reaches it: `this_machine`, `local_network`, `public_internet`, `via_gateway`, or `unknown`. Derived from the address's scope and the subnets of the machine's own interfaces that are up, never from an input, and never by resolving a name; `unknown` is a stop where a place decides something ([decision 0313](../decisions/0313-the-place-of-an-address-is-derived-from-its-scope-and-the-machines-subnets.md)) |
| Address scope | What kind of place an address is, read from its literal alone and without asking the machine or a resolver: `this_machine`, `unspecified`, `link_local`, `private_use`, `carrier_grade_nat`, `unique_local`, `multicast`, `documentation`, `reserved`, `public`, `name`, or `unreadable`. It is not the place of [decision 0223](../decisions/0223-a-network-is-a-place-a-recognized-network-and-a-reach.md), which also needs the machine's own network state ([decision 0240](../decisions/0240-one-typed-reading-of-where-an-address-is-and-one-question-of-a-plain-link.md)) |
| Application's verdict | The system's own answer to a write that was sent: it saved, or it refused or found the record changed. Only a verdict settles a sent write; an answer that is not one, such as a redirect, a reply too large to read, or a refusal made before the system looked, leaves the write in doubt until it is verified |
| Approval | A person's decision to allow one exact diff, stored as a single-use record |
| Archetype | One of the seven ways a system is reached: interface, request, command, bridge, artifact, stream, device. Fixes how a step is acted, confirmed, and read back |
| Artifact | A large piece of evidence: a snapshot, a network exchange, a screenshot. Also the name of the archetype that reads and writes files; context makes clear which |
| Asserted read | An endpoint marked `read` whose method may write, with `read_asserted: true`: a person's statement that the request changes nothing. The HTTP pack sends it outside a live run only from a published version, against an environment of kind `test` ([decision 0270](../decisions/0270-the-http-pack-sends-an-asserted-read-only-from-a-published-version-and-bounds-what-it-leaves-running.md)); the web pack is to let a page send it in an observing tab under the same rule ([decision 0392](../decisions/0392-a-page-that-reads-by-a-method-that-may-write-is-learned-from-a-persons-watched-use-and-read-through-an-asserted-request.md), not built) |
| Baseline | For a watch, the keyed digest of each record's values and of each output, as of the last check that saw the record. A check is compared with it, not with the check before ([decision 0247](../decisions/0247-a-watch-keeps-only-digests-and-its-due-times-come-from-the-stores-clock.md)) |
| Binding | How one mode carries out one operation. An operation can have several |
| Binding Selector | The part of the engine loop that chooses which binding of an operation runs: the first verified binding whose driver is ready and whose surface has not drifted, with the reason recorded |
| Bridge | Code that runs inside a host application or process, through an interface the host offers, and speaks to the runtime from there |
| Build brief | What a lead hands a worker for one package: the goal, the proof, the files it owns, and the checks to run. Kept outside the repository ([decision 0326](../decisions/0326-plan-names-only-the-compiled-plan-and-the-build-document-is-the-build-order.md)) |
| Build Order | The document of what is left to build, in what order, and how far the build has come: [docs/plan/build-order.md](../plan/build-order.md) (decision 0326). Not the compiled target |
| Candidate | A published version being compared in shadow before it becomes current |
| Blob | Any stored evidence or value, named by a keyed digest of its content and sealed |
| Capture | A recording of a system's traffic that yields structure and never a value, through the provider contract's one check. The scout takes one of a page's own requests in an observing tab. A capture of a person's use is driven by that person, who uses the system while the pack listens. An agent may ask for one; only its person begins and ends it, on the engine's own page. For the web pack it runs in the watching tab. Designed ([decision 0392](../decisions/0392-a-page-that-reads-by-a-method-that-may-write-is-learned-from-a-persons-watched-use-and-read-through-an-asserted-request.md)); not built |
| Change (of a watch) | One difference a check reports, kept in the ledger as the kind, the keyed digest of the record's key, and the names of the outputs that differ, never a value. The kinds are `confirmed`, `added`, `changed`, `removed`, `unseen`, `gap`, `paused`, `resumed`, and `ended` ([decision 0247](../decisions/0247-a-watch-keeps-only-digests-and-its-due-times-come-from-the-stores-clock.md)) |
| Check (of a watch) | One call of a watch's read: an ordinary run of that read, compared with the baseline when it completes ([decision 0234](../decisions/0234-a-watch-is-a-read-repeated-on-a-schedule-a-person-confirmed-and-a-change-is-reported-never-acted-on.md)) |
| Client | Anything that talks to the engine: a command, an MCP client, an SDK, the tray application |
| Commit point | One change the system makes durable: a save, a committed transaction, a replaced file, a published message. One ledger row |
| Compiled target | The compiled, hashed form of a target version that the engine runs: what the compiler makes from a target's files, run step by step with no model. Formerly "plan" ([decision 0329](../decisions/0329-the-compiled-file-the-engine-runs-is-the-compiled-target.md)) |
| Control | Everything the engine asks of the control plane while it runs a job: the compiled target, the policy decision, a session lease, a record lock, a budget, the approval check, recording an intent, sending events. In one process it is a direct call; on a runner it is the runner link |
| Control plane | The part that decides, records, and hands out work |
| Coverage | How much of what could have been looked at was looked at: complete, partial, or unknown. An empty result with anything less than complete coverage is not absence |
| Crate design | The page in `docs/architecture/crates/` that says what one crate owns, offers, uses, stores, and must never do |
| Creation lock | An exclusive operating-system lock taken while something that must exist once is made, such as an installation's root key or the passphrase route's salt, so that two engines starting together make one |
| Decision point | A question a person may have to answer during a run, declared by the operation with a name and its wording. A run asks only what its operation declares, and waits at `needs_input` |
| Diff | Current values against proposed values, record by record, shown before a write |
| Discoverer | The extension point that inspects a target and returns findings. It can never write |
| Distinct input (of a shadow binding) | One input of a call, told apart from another by the workspace's keyed digest of the call's inputs. Two calls with equal inputs are one distinct input. A shadow binding is promotable when, in every environment its evidence must come from, the distinct inputs that matched the verified binding since the last mismatch number at least the threshold ([decision 0251](../decisions/0251-the-evidence-for-promoting-a-shadow-binding-says-what-it-covers-and-a-shadow-costs-what-it-says.md)) |
| Diff hash | A hash of a diff in canonical form; what an approval is bound to |
| Digest key | An installation's key for keyed digests. The vault makes it at first use and keeps it wrapped by the root key; each workspace's key for digests is derived from it and never stored |
| Draft | Target files not yet published; runnable only in the sandbox |
| Drafter | The kernel code that writes a draft from a finding wherever a pack's drafting rules say how. Its drafts are ordinary drafts |
| Drifting | What a canary reports when a field or action still passes but the locator that matched it is not the first in its list. The tool stays available; it is the warning before `drift` ([decision 0323](../decisions/0323-a-repair-is-proposed-by-a-fixed-scorer-and-drifting-is-seen-before-drift.md)) |
| Driver | The code that carries out steps in one mode |
| Earlier run | An unfinished run (running, resting for a person, or stopped and resumable) that an identical call is given back instead of a new run; the result is marked `earlier_run` ([decision 0176](../decisions/0176-the-action-registry-takes-up-the-kernels-second-pass.md)) |
| Effect class | `read`, `write`, or `irreversible` |
| Endpoint catalog | Where every request the HTTP pack sends is stated, once: the endpoint entries in `endpoints/`, and the pinned descriptions in `descriptions/`, whose documents are the entries of reads bound in `api` ([decision 0389](../decisions/0389-the-api-mode-binds-a-read-to-a-pinned-description-and-names-its-change-as-drift.md)) |
| Engine | The Anything2MCP process. On one machine, one per user |
| Engine loop | The eight parts every operation call goes through: guard, choose, prepare, intend, act, confirm, read back, record |
| Envelope | A sealed value together with what it is bound to: the owner's key and its generation, the wrapped data key, and the ciphertext. It opens only under the context it was sealed for |
| Environment | A named instance of a target, such as test or production |
| Evidence | Everything recorded about a run or an observation: events, step trace, artifacts, and what a discoverer saw |
| Evidence level | How far a finding, draft, or tool has been proven: `inferred`, `discovered`, `reconstructed`, `schema_verified`, `executed`, `behavior_verified`, `approved` |
| Exclusive resource | Something only one job may hold at a time, such as a person's screen or a device link, named by a key. The scheduler hands out no job while another job holds one of its keys |
| Extension point | One of the fixed kinds of thing a provider can supply: driver, auth provider, source reader, widget driver, discoverer |
| Failure class | The category of a stop, which alone decides what happens next |
| Field test | One run of the product against one copy of existing software that the project runs itself, holding invented data, started and checked as a recipe says; the software's own store is the proof ([decision 0295](../decisions/0295-a-field-test-is-run-from-a-recipe-kept-outside-the-repository.md)). Not a trial, which is one draft run in the sandbox |
| Finding | Something learned about a target, with an evidence level, its subject, and its limitations. Never an operation |
| First binding | The first binding of an operation, drafted as `draft`; `binding.promote` makes it able to act once the operation's offline tests pass and a scout proved it, and for a write a person does it ([decision 0189](../decisions/0189-a-developers-first-hour-runs-without-hand-steps.md)) |
| Fixed place | A record that is one place and has no key: one record of a JSON document at a path, one section or dictionary of a settings file, or a named cell. An operation writes it with `identity: {strategy: natural, place: fixed}` and no identity input, where the pack's file kind lists the place as fixed ([decision 0252](../decisions/0252-a-fixed-place-a-review-that-shows-what-runs-and-a-sign-in-before-its-probe.md)) |
| Fixture | Scrubbed recorded evidence used for offline tests |
| Fixture record | The action `fixture.record`: it runs a read once in the sandbox and writes an offline test and its fixture, with every observed value replaced by a made-up one of the same kind; with no rule for a value it stops and writes nothing ([decision 0196](../decisions/0196-the-engine-and-the-registry-after-the-acknowledgement-and-the-first-hour.md)) |
| Gap | A change of a watch that says a check did not complete, or that due times passed while the engine was not running. A missed due time is never made up by a later check ([decision 0247](../decisions/0247-a-watch-keeps-only-digests-and-its-due-times-come-from-the-stores-clock.md)). A gap is also written for a check whose run's own result is not of the watch's record shape (`watch_result_not_its_shape`), and for the check a watch's end closes while it runs (`watch_ended`) ([decision 0262](../decisions/0262-watches-are-held-to-their-end-their-budget-and-their-runs-own-result.md)) |
| Gate evidence | What each publish gate returned when a version was published, stored with the version, with the gates that did not run named as such ([decision 0180](../decisions/0180-a-targets-offline-tests-cover-writes-and-gate-the-publish.md)) |
| Generation | Which version of a key: 1 for the first, one more after each rotation |
| Grant | A rule giving a principal access to a scope up to an effect class |
| Guardrail | A product rule that is compiled in and that no setting, profile, or preset can loosen |
| Hazard | What an operation could damage beyond the data it names: nothing, data elsewhere, something sent outside, a device's ability to work, or the physical world |
| Held folder | A folder reached from its root down with no link followed, and held while a pack acts on one entry directly in it, so that nothing done to the path meanwhile leads the act elsewhere: on Linux and macOS its own descriptor, on Windows every folder from the root down held open. Made by `CheckedPath::hold_folder` ([decision 0272](../decisions/0272-the-file-packs-folder-writes-act-through-a-held-folder-after-a-second-reader.md)) |
| Hold | The scheduler's stop on a scope: everything, a workspace, a target, or a caller. No job in a held scope is handed out. It is how a kill switch reaches the scheduler |
| Holding folder | The folder `held/` inside a target's folder, where the drafter writes a draft that holds an open question, with every file drafted from the same finding. The loader does not read it, so it is in no compiled target ([decision 0298](../decisions/0298-a-draft-with-an-open-question-waits-in-a-holding-folder.md)). Not a held folder, which is a folder kept open while a pack acts on one entry |
| Repair proposer | A pack's deterministic scorer that ranks candidate elements in a failure snapshot under fixed weights, for the repairing agent and the reviewer; it proposes nothing when the top two are too close, and applies nothing (decision 0323) |
| Source Definitions | The files under a target's `sources/` folder: how a kind of handed file is read and mapped. Compiled with the target ([decision 0309](../decisions/0309-two-names-are-settled-the-source-folder-and-what-guard-checks.md)). |
| Handed Files | The files in the project's `sources` folder: what a person gives a scout or a pipeline. Named from the project folder and never compiled. |
| Field harness | A program of the workspace under `tools/field-harness/`, built from the product crate's library: the product's own `serve` with the field prompt. Runs only in a container network with no route out, beside the copies a field test starts, and is never packaged ([decision 0310](../decisions/0310-the-field-harness-runs-in-a-closed-network-and-says-in-the-ledger-what-it-is.md)). |
| Field prompt | The presence prompt of the field harness. It answers on a pipe the field test's runner holds, passes only the short hash the runner sends, refuses an irreversible operation, and is recorded in the ledger as `field_harness`. |
| Telemetry | The old name of logs and traces ([decision 0330](../decisions/0330-every-name-says-what-the-part-is-in-plain-words.md)). The settings keys `telemetry.export` and `telemetry.crash_reports` keep the word, as keys a person writes in a settings file. Package N0b renamed it in the code and the documents in force; what is stored keeps it, as [decision 0402](../decisions/0402-the-rename-sweep-renames-every-name-and-keeps-what-is-stored.md) says |
| Waiting read | A drafted read that was not sent because its endpoint takes a session's credential and no one has signed in. Reported apart from a proven and a failed read ([decision 0303](../decisions/0303-a-declared-sign-in-is-kept-and-a-read-that-waits-for-one-is-reported-as-waiting.md)). |
| Home runner | The runner holding a session's live state |
| Health state | `ok`, `degraded`, or `stopped`: what the engine can currently do |
| Idempotency key | Text a caller sends with `tool.call` so that the same call made again gives back its first run and starts no other; the same key with another call is refused ([decision 0160](../decisions/0160-an-idempotency-key-gives-back-its-run-and-run-list-shows-a-callers-own-runs.md)) |
| Identity | Who the target sees: a person, or a dedicated account |
| Identity strategy | How a write's record is found again: natural, composite, stamped, find, or none |
| Identity mode | How that identity is obtained: per-user token, per-user session, per-user device, or service account |
| In doubt | Said of a record whose write may have happened: its intent was stored and nothing yet proves the outcome. Nothing more is written for it until reconcile has read the system |
| Installation name | The name of one installation, which names the root key's entry in the operating system's key store. Never a target's name |
| Intent | A ledger entry made before a write, recording what is about to be sent |
| Intent token | What the ledger returns when an intent is recorded. A driver's handle refuses a step that commits without one |
| Job | A piece of a run handed to a runner under a lease |
| Kernel | The crates that are the same whatever kind of system is integrated |
| Key record | The stored form of one wrapped key: its identifier, layer, and generation, the wrapped key, and which key wrapped it. One per key and generation, kept by the ledger |
| Kill switch | A person's stop on everything, a workspace, a target, or an operation, kept by the policy engine. While it is on, a call in its scope is refused with `kill_switch_on` |
| Lease | Time-limited permission for one runner to work on one job, with a number that only increases |
| Local channel | The per-user socket or named pipe between clients and the engine on one machine |
| Lock ticket | What a run takes from the scheduler before it asks the ledger for a record lock. A wait that starts from it returns at once if the lock was freed since, so no release is missed |
| Logs and traces | What the engine writes about itself on this machine: its logs, spans, and metrics, and the step trace and its captures, each passed through the redactor first. Written by the crate `a2m-logs`; nothing is sent anywhere |
| Managed component | An outside program a provider needs, detected and identified by digest, never bundled and never fetched during a run |
| Manifest | A provider's declaration of what it supplies, reaches, needs, and may do |
| Matched screen | One screen found from several pages whose paths differ in one segment and whose snapshots agree; its address holds a placeholder for that segment ([decision 0187](../decisions/0187-the-web-pack-matches-a-screen-by-its-address-and-reads-by-key.md)) |
| Mapping file | The file that names which tools of an upstream MCP server may be reached, and as what. No other tool of that server is reachable |
| Mode | One concrete way of reaching a system, brought by a pack and belonging to one archetype: `ui`, `http`, and `mcp` are three. The kernel holds no list of modes; a provider registers each one it brings |
| Next step (of a failure) | The instruction a failure carries on what to do about it. Each code's general one is written once, in its row of the crate's `failure_codes!` table; a failure made for a case may carry a more exact one ([decision 0258](../decisions/0258-every-crate-declares-its-failure-codes-in-one-table.md)) |
| Observing tab | A browser tab that cannot write, in which every web read runs in every posture; a write's own steps run in an acting tab ([decision 0195](../decisions/0195-a-read-never-runs-where-the-page-may-write-and-a-page-keeps-only-its-structure.md)) |
| Open question | The marker the drafter leaves in a draft where a value is needed that the system did not state, such as a unit or a key. The compiler refuses it in every mode, and the question goes to the unknowns ledger |
| Operation | One business action with typed inputs and outputs, defined once |
| Owner | The person a user session belongs to |
| Owner mark | A value the supervisor places in a child process's environment so that it can prove the process is its own before stopping it |
| Pack | The crates that know one kind of system. Compiled into the binary; enabled by a setting |
| Page request | A request a web page makes by itself, which the observing tab records as names and kinds and which a `page_http` read may repeat from inside the page, with GET alone ([decision 0190](../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md)) |
| Part | One of the seven divisions of the specification; the first digit of a section's number |
| Person's act | An action whose definition marks it as a person's, such as publishing a version. When an agent asks for one, the person is shown a page of exactly what will be done, and only their press and proof of presence carry it out ([decision 0163](../decisions/0163-a-persons-act-an-agent-asks-for-is-carried-out-on-a-page-of-the-person-surface.md)) |
| Pinned description | The HTTP pack's reading of a published description's drafted reads, in `descriptions/`, with the digest of the text it was read from and where it was served; an `api` binding names one, and its change is `drift`, `description_changed` ([decision 0389](../decisions/0389-the-api-mode-binds-a-read-to-a-pinned-description-and-names-its-change-as-drift.md)) |
| Pipeline | Operations in order over a source, with checkpoints |
| Pipeline step | One entry in a pipeline's `steps`: one operation, called once or once per row. Never called a step alone |
| Plan | The old name of the compiled target ([decision 0329](../decisions/0329-the-compiled-file-the-engine-runs-is-the-compiled-target.md)). Package N0 renamed it in the code and the documents; the stored names that keep it are listed in [decision 0402](../decisions/0402-the-rename-sweep-renames-every-name-and-keeps-what-is-stored.md) |
| Policy set | The versioned rules the policy engine evaluates |
| Pre-approval rule | A rule in a policy set under which a write may proceed without a person, for one run and one diff. It can never cover an `irreversible` operation or a hazard other than `none` |
| Precondition | An input of a writing operation that the write is sent with and the system checks, and that is no field of the record, such as the version a guard compares. Declared `precondition: true` on the input; shown in the diff and bound by the approval, never compared with a read-back ([decision 0378](../decisions/0378-an-input-marked-precondition-is-sent-with-a-write-and-never-compared-with-its-read-back.md)) |
| Preset | A named bundle of settings a person picks instead of setting each one. A preset may only tighten |
| Principal | Anyone or anything that calls Anything2MCP |
| Proper name | The one name of a part: a noun or a short noun phrase from this glossary, from the code, or coined. A description is not a name ([decision 0301](../decisions/0301-every-part-has-a-proper-name.md)) |
| Protected setting | A setting that only a person can loosen, through a channel an agent cannot reach: on one machine a user-presence prompt; on a server the operator's file or the console after sign-in |
| Provider | Anything that supplies capabilities to the runtime: a pack, a plug-in, or an upstream MCP server |
| Probe | For a session: a cheap signed-in read. For a target: the first inspection of a system's surfaces |
| Profile | A deployment shape, such as Solo or Company server. Also a browser's data directory; context makes clear which |
| Project | A folder with a `anything2mcp.toml` holding target files |
| Project root | The root of a run's roots that stands for the project folder of each request, so that one engine serves many projects without one reading another's files ([decision 0193](../decisions/0193-the-core-the-contract-the-compiler-and-the-scheduler-after-the-second-pass.md), [decision 0194](../decisions/0194-the-packs-the-product-and-the-local-surface-take-up-the-kernels-second-pass.md)) |
| Publish gate | A check a version must pass before it is published. Two of the twelve in section 403 are built, the schema and lint check and the offline replay ([decision 0180](../decisions/0180-a-targets-offline-tests-cover-writes-and-gate-the-publish.md)) |
| Quarantine | The wait after a lost lease before the run's records may be touched |
| Quarantined | Said of a record lock that is kept, not released, because its record is in doubt. Only its own run, under a new lease, can confirm it |
| Read-back | Reading a value from the system after writing it |
| Recipe | The file a field test is run from: the software's image pinned by digest, the invented data, each command, and what each must show. It names real software, so it is kept outside this repository |
| Reconcile | Reading doubtful records back to learn whether their writes landed |
| Record | One item in the system that a run reads or writes, identified by a key. A ledger row is a commit point, which usually carries one record but can carry several, or part of one |
| Registered name | A word whose values arrive with packs and crates, checked for shape and registered by whoever brings it: a mode, a surface kind, a step kind, a file kind, a lint code, a finding kind, a control name, a failure code. Its shape, which a target's, an environment's, and every other checked name's share, is a lowercase letter followed by lowercase letters, digits, and underscores; a name Windows keeps for a device (`con`, `prn`, `aux`, `nul`, `com0` to `com9`, `lpt0` to `lpt9`) is refused on every system, as is a dotted name with one as a part ([decision 0431](../decisions/0431-a-name-that-windows-keeps-for-a-device-is-refused-on-every-system.md)) |
| Replay | A driver of recorded answers that every pack gets from the contract; an offline test runs a tool's steps against a fixture through it, and nothing leaves the machine ([decision 0177](../decisions/0177-a-targets-offline-tests-run-against-a-replay-of-their-fixtures.md)) |
| Review summary | The risk-ordered list a person sees before publishing or importing a version |
| Root | A named folder a pack may reach, declared in its manifest for reading or for writing. A pack opens a file only by a checked path under one of its roots, never by a free path |
| Run | One call of an operation or pipeline, identified by a `run_id` |
| Scenario | A short, declared sequence of named steps that exercises one thing in a target. The unit of observation during discovery |
| Runner | The part that executes steps and holds live sessions |
| Scout | The fixed pipeline that identifies a target, runs its discoverers, drafts what follows by rule, tries the drafted reads, and reports. It runs no model and never tries a write |
| Screen model | A named page or window: how to reach it, its fields, and how to know it is ready |
| Session | The signed-in state of one identity on one target in one environment |
| Session keeper | The only part that signs in, checks, renews, and ends sessions |
| Shadow | A binding that predicts what it would do without acting, to earn promotion |
| Sign-in mode | How an entry of a target's `auth.yaml` says a session is obtained: `use_existing`, `managed_session`, or `custom` |
| Snapshot | A compact view of the current page or window, built from the accessibility tree |
| Source | A reader that turns a file or another system's output into validated records |
| Stdio proxy | The small program an MCP client starts with `anything2mcp bridge`, which carries MCP messages between standard input and output and the running engine and holds no logic. Not a bridge, which is an archetype |
| Step | One fixed instruction in a compiled target, carried out by a driver |
| Step trace | One structured record per step of a run |
| Subject | What a finding or a piece of evidence is about, as one tagged identity: a file by digest, an address, a process, a session, a device, or unknown with a reason |
| Supervisor | The part of the kernel that starts, talks to, and stops outside processes and connections, and stops only what it can prove it started |
| Surface | One way into a target: a web origin, an API base address, an MCP server, a desktop window, a program, a database, a file, a broker, a device link |
| Surface notice | A fixed sentence a surface's kind registers for a person to see wherever the surface is shown, such as that credentials go unencrypted ([decision 0193](../decisions/0193-the-core-the-contract-the-compiler-and-the-scheduler-after-the-second-pass.md)) |
| Target | One system being wrapped: an application, a service, a program, a data store, a device |
| Template version | One version of a source's template that a source definition knows, with the fingerprint of its structure. An input that matches no template version is refused |
| Test stand | The piece of test equipment that imitates one kind of system and reports ground truth through a control interface. The test application is the first |
| Throwaway store | An in-memory ledger made for one offline test, with the same migrations and rules as every store, gone when the test ends; only it takes the offline-test clearance ([decision 0180](../decisions/0180-a-targets-offline-tests-cover-writes-and-gate-the-publish.md)) |
| Tool | An operation or pipeline as an agent sees it |
| Toolset | A named, versioned selection of tools with its own endpoint, members, and policy |
| Trace signature | A hash of the parts of a step trace that should not vary between identical runs |
| Vault | Encrypted storage for sessions, credentials, and evidence |
| Version | An immutable, tested, published snapshot of a target |
| Watch | One published read, called with one fixed input at a fixed interval until an end the person confirmed; it reports each difference between checks and never acts on one ([decision 0234](../decisions/0234-a-watch-is-a-read-repeated-on-a-schedule-a-person-confirmed-and-a-change-is-reported-never-acted-on.md)) |
| Watching tab | A tab in a visible window of the engine's own browser in which a person uses a system while the engine only listens: it has no input method, sends nothing, and fails no request, since every request there is the person's. A capture runs in it. Not built ([decision 0392](../decisions/0392-a-page-that-reads-by-a-method-that-may-write-is-learned-from-a-persons-watched-use-and-read-through-an-asserted-request.md)) |
| Window | The finding kind of the desktop pack: the shape of one window of a running application (roles, structure, and the names of controls, tables, and headings, never a cell or a value), from which a screen and reads are drafted ([decision 0198](../decisions/0198-the-desktop-pack-reads-linux-applications-through-a-look-only-back-end.md)) |
| Workspace | An isolation boundary inside one installation |
