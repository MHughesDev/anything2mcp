# Glossary

One meaning per word. Code, files, messages, and documents use these terms and no synonyms. A word here names a thing whether or not that thing is built yet.

| Term | Meaning |
| --- | --- |
| Absence steps | The step kinds a mode lists as authoritative lookups. Only a lookup made of them may prove that a write of a record with no key did not land, and only when it answers "absent" twice, a settle time apart ([decision 0185](../decisions/0185-a-record-with-no-key-is-never-proved-absent-by-one-look.md)) |
| Acknowledged | The weakest proof a write can end on: the system answered the committing step and nothing could be read back. The record is `acted`, never verified, and only an operation declared `irreversible` may offer it ([decision 0172](../decisions/0172-the-ledger-ends-a-record-on-an-acknowledgement-and-keeps-what-the-system-said.md)) |
| Action | An authoring or administration function defined once in the action registry and exposed as a command, an MCP tool, a REST route, and an SDK method |
| Address scope | What kind of place an address is, read from its literal alone and without asking the machine or a resolver: `this_machine`, `unspecified`, `link_local`, `private_use`, `carrier_grade_nat`, `unique_local`, `multicast`, `documentation`, `reserved`, `public`, `name`, or `unreadable`. It is not the place of [decision 0223](../decisions/0223-a-network-is-a-place-a-recognized-network-and-a-reach.md), which also needs the machine's own network state ([decision 0240](../decisions/0240-one-typed-reading-of-where-an-address-is-and-one-question-of-a-plain-link.md)) |
| Application's verdict | The system's own answer to a write that was sent: it saved, or it refused or found the record changed. Only a verdict settles a sent write; an answer that is not one, such as a redirect, a reply too large to read, or a refusal made before the system looked, leaves the write in doubt until it is verified |
| Approval | A person's decision to allow one exact diff, stored as a single-use record |
| Archetype | One of the seven ways a system is reached: interface, request, command, bridge, artifact, stream, device. Fixes how a step is acted, confirmed, and read back |
| Artifact | A large piece of evidence: a snapshot, a network exchange, a screenshot. Also the name of the archetype that reads and writes files; context makes clear which |
| Baseline | For a watch, the keyed digest of each record's values and of each output, as of the last check that saw the record. A check is compared with it, not with the check before ([decision 0247](../decisions/0247-a-watch-keeps-only-digests-and-its-due-times-come-from-the-stores-clock.md)) |
| Binding | How one mode carries out one operation. An operation can have several |
| Bridge | Code that runs inside a host application or process, through an interface the host offers, and speaks to the runtime from there |
| Candidate | A published version being compared in shadow before it becomes current |
| Blob | Any stored evidence or value, named by a keyed digest of its content and sealed |
| Change (of a watch) | One difference a check reports, kept in the ledger as the kind, the keyed digest of the record's key, and the names of the outputs that differ, never a value. The kinds are `confirmed`, `added`, `changed`, `removed`, `unseen`, `gap`, `paused`, `resumed`, and `ended` ([decision 0247](../decisions/0247-a-watch-keeps-only-digests-and-its-due-times-come-from-the-stores-clock.md)) |
| Check (of a watch) | One call of a watch's read: an ordinary run of that read, compared with the baseline when it completes ([decision 0234](../decisions/0234-a-watch-is-a-read-repeated-on-a-schedule-a-person-confirmed-and-a-change-is-reported-never-acted-on.md)) |
| Client | Anything that talks to the engine: a command, an MCP client, an SDK, the tray application |
| Commit point | One change the system makes durable: a save, a committed transaction, a replaced file, a published message. One ledger row |
| Control | Everything the engine asks of the control plane while it runs a job: the plan, the policy decision, a session lease, a record lock, a budget, the approval check, recording an intent, sending events. In one process it is a direct call; on a runner it is the runner link |
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
| Driver | The code that carries out steps in one mode |
| Earlier run | An unfinished run (running, resting for a person, or stopped and resumable) that an identical call is given back instead of a new run; the result is marked `earlier_run` ([decision 0176](../decisions/0176-the-action-registry-takes-up-the-kernels-second-pass.md)) |
| Effect class | `read`, `write`, or `irreversible` |
| Engine | The Anything2MCP process. On one machine, one per user |
| Engine loop | The eight parts every operation call goes through: guard, choose, prepare, intend, act, confirm, read back, record |
| Envelope | A sealed value together with what it is bound to: the owner's key and its generation, the wrapped data key, and the ciphertext. It opens only under the context it was sealed for |
| Environment | A named instance of a target, such as test or production |
| Evidence | Everything recorded about a run or an observation: events, step trace, artifacts, and what a discoverer saw |
| Evidence level | How far a finding, draft, or tool has been proven: `inferred`, `discovered`, `reconstructed`, `schema_verified`, `executed`, `behavior_verified`, `approved` |
| Exclusive resource | Something only one job may hold at a time, such as a person's screen or a device link, named by a key. The scheduler hands out no job while another job holds one of its keys |
| Extension point | One of the fixed kinds of thing a provider can supply: driver, auth provider, source reader, widget driver, discoverer |
| Failure class | The category of a stop, which alone decides what happens next |
| Finding | Something learned about a target, with an evidence level, its subject, and its limitations. Never an operation |
| First binding | The first binding of an operation, drafted as `draft`; `binding.promote` makes it able to act once the operation's offline tests pass and a scout proved it, and for a write a person does it ([decision 0189](../decisions/0189-a-developers-first-hour-runs-without-hand-steps.md)) |
| Fixture | Scrubbed recorded evidence used for offline tests |
| Fixture record | The action `fixture.record`: it runs a read once in the sandbox and writes an offline test and its fixture, with every observed value replaced by a made-up one of the same kind; with no rule for a value it stops and writes nothing ([decision 0196](../decisions/0196-the-engine-and-the-registry-after-the-acknowledgement-and-the-first-hour.md)) |
| Gap | A change of a watch that says a check did not complete, or that due times passed while the engine was not running. A missed due time is never made up by a later check ([decision 0247](../decisions/0247-a-watch-keeps-only-digests-and-its-due-times-come-from-the-stores-clock.md)) |
| Gate evidence | What each publish gate returned when a version was published, stored with the version, with the gates that did not run named as such ([decision 0180](../decisions/0180-a-targets-offline-tests-cover-writes-and-gate-the-publish.md)) |
| Generation | Which version of a key: 1 for the first, one more after each rotation |
| Grant | A rule giving a principal access to a scope up to an effect class |
| Guardrail | A product rule that is compiled in and that no setting, profile, or preset can loosen |
| Hazard | What an operation could damage beyond the data it names: nothing, data elsewhere, something sent outside, a device's ability to work, or the physical world |
| Hold | The scheduler's stop on a scope: everything, a workspace, a target, or a caller. No job in a held scope is handed out. It is how a kill switch reaches the scheduler |
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
| Managed component | An outside program a provider needs, detected and identified by digest, never bundled and never fetched during a run |
| Manifest | A provider's declaration of what it supplies, reaches, needs, and may do |
| Matched screen | One screen found from several pages whose paths differ in one segment and whose snapshots agree; its address holds a placeholder for that segment ([decision 0187](../decisions/0187-the-web-pack-matches-a-screen-by-its-address-and-reads-by-key.md)) |
| Mapping file | The file that names which tools of an upstream MCP server may be reached, and as what. No other tool of that server is reachable |
| Mode | One concrete way of reaching a system, brought by a pack and belonging to one archetype: `ui`, `http`, and `mcp` are three. The kernel holds no list of modes; a provider registers each one it brings |
| Observing tab | A browser tab that cannot write, in which every web read runs in every posture; a write's own steps run in an acting tab ([decision 0195](../decisions/0195-a-read-never-runs-where-the-page-may-write-and-a-page-keeps-only-its-structure.md)) |
| Open question | The marker the drafter leaves in a draft where a value is needed that the system did not state, such as a unit or a key. The compiler refuses it in every mode, and the question goes to the unknowns ledger |
| Operation | One business action with typed inputs and outputs, defined once |
| Owner | The person a user session belongs to |
| Owner mark | A value the supervisor places in a child process's environment so that it can prove the process is its own before stopping it |
| Pack | The crates that know one kind of system. Compiled into the binary; enabled by a setting |
| Page request | A request a web page makes by itself, which the observing tab records as names and kinds and which a `page_http` read may repeat from inside the page, with GET alone ([decision 0190](../decisions/0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md)) |
| Part | One of the seven divisions of the specification; the first digit of a section's number |
| Person's act | An action whose definition marks it as a person's, such as publishing a version. When an agent asks for one, the person is shown a page of exactly what will be done, and only their press and proof of presence carry it out ([decision 0163](../decisions/0163-a-persons-act-an-agent-asks-for-is-carried-out-on-a-page-of-the-person-surface.md)) |
| Pipeline | Operations in order over a source, with checkpoints |
| Pipeline step | One entry in a pipeline's `steps`: one operation, called once or once per row. Never called a step alone |
| Plan | The compiled, hashed form of a target version that the engine runs |
| Policy set | The versioned rules the policy engine evaluates |
| Pre-approval rule | A rule in a policy set under which a write may proceed without a person, for one run and one diff. It can never cover an `irreversible` operation or a hazard other than `none` |
| Preset | A named bundle of settings a person picks instead of setting each one. A preset may only tighten |
| Principal | Anyone or anything that calls Anything2MCP |
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
| Reconcile | Reading doubtful records back to learn whether their writes landed |
| Record | One item in the system that a run reads or writes, identified by a key. A ledger row is a commit point, which usually carries one record but can carry several, or part of one |
| Registered name | A word whose values arrive with packs and crates, checked for shape and registered by whoever brings it: a mode, a surface kind, a step kind, a file kind, a lint code, a finding kind, a control name, a failure code |
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
| Step | One fixed instruction in a compiled plan, carried out by a driver |
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
| Window | The finding kind of the desktop pack: the shape of one window of a running application (roles, structure, and the names of controls, tables, and headings, never a cell or a value), from which a screen and reads are drafted ([decision 0198](../decisions/0198-the-desktop-pack-reads-linux-applications-through-a-look-only-back-end.md)) |
| Workspace | An isolation boundary inside one installation |
