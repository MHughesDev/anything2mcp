# Glossary

One meaning per word. Code, files, messages, and documents use these terms and no synonyms.

| Term | Meaning |
| --- | --- |
| Action | An authoring or administration function defined once in the action registry and exposed as a command, an MCP tool, a REST route, and an SDK method |
| Approval | A person's decision to allow one exact diff, stored as a single-use record |
| Archetype | One of the seven ways a system is reached: interface, request, command, bridge, artifact, stream, device. Fixes how a step is acted, confirmed, and read back |
| Artifact | A large piece of evidence: a snapshot, a network exchange, a screenshot. Also the name of the archetype that reads and writes files; context makes clear which |
| Binding | How one mode carries out one operation. An operation can have several |
| Bridge | Code that runs inside a host application or process, through an interface the host offers, and speaks to the runtime from there |
| Candidate | A published version being compared in shadow before it becomes current |
| Blob | Any stored evidence or value, named by its content hash and encrypted |
| Client | Anything that talks to the engine: a command, an MCP client, an SDK, the tray application |
| Commit point | One save in the application. One ledger row |
| Control plane | The part that decides, records, and hands out work |
| Coverage | How much of what could have been looked at was looked at: complete, partial, or unknown. An empty result with anything less than complete coverage is not absence |
| Diff | Current values against proposed values, record by record, shown before a write |
| Discoverer | The extension point that inspects a target and returns findings. It can never write |
| Diff hash | A hash of a diff in canonical form; what an approval is bound to |
| Draft | Target files not yet published; runnable only in the sandbox |
| Drafter | The kernel code that writes a draft from a finding wherever a pack's drafting rules say how. Its drafts are ordinary drafts |
| Driver | The code that carries out steps in one mode |
| Effect class | `read`, `write`, or `irreversible` |
| Engine | The Anything2MCP process. On one machine, one per user |
| Engine loop | The eight parts every operation call goes through: guard, choose, prepare, intend, act, confirm, read back, record |
| Environment | A named instance of a target, such as test or production |
| Evidence | Everything recorded about a run or an observation: events, step trace, artifacts, and what a discoverer saw |
| Evidence level | How far a finding, draft, or tool has been proven: `inferred`, `discovered`, `reconstructed`, `schema_verified`, `executed`, `behavior_verified`, `approved` |
| Extension point | One of the fixed kinds of thing a provider can supply: driver, auth provider, source reader, widget driver, discoverer |
| Failure class | The category of a stop, which alone decides what happens next |
| Finding | Something learned about a target, with an evidence level, its subject, and its limitations. Never an operation |
| Fixture | Scrubbed recorded evidence used for offline tests |
| Grant | A rule giving a principal access to a scope up to an effect class |
| Hazard | What an operation could damage beyond the data it names: nothing, data elsewhere, something sent outside, a device's ability to work, or the physical world |
| Home runner | The runner holding a session's live state |
| Health state | `ok`, `degraded`, or `stopped`: what the engine can currently do |
| Identity | Who the target application sees |
| Identity strategy | How a write's record is found again: natural, composite, stamped, find, or none |
| Identity mode | How that identity is obtained: per-user token, per-user session, per-user device, or service account |
| Intent | A ledger entry made before a write, recording what is about to be sent |
| Job | A piece of a run handed to a runner under a lease |
| Kernel | The crates that are the same whatever kind of system is integrated |
| Lease | Time-limited permission for one runner to work on one job, with a number that only increases |
| Local channel | The per-user socket or named pipe between clients and the engine on one machine |
| Managed component | An outside program a provider needs, detected and identified by digest, never bundled and never fetched during a run |
| Manifest | A provider's declaration of what it supplies, reaches, needs, and may do |
| Mode | One of the seven ways to reach an application: `ui`, `page_http`, `http`, `api`, `site_tools`, `mcp`, `desktop` |
| Operation | One business action with typed inputs and outputs, defined once |
| Owner | The person a user session belongs to |
| Pack | The crates that know one kind of system. Compiled into the binary; enabled by a setting |
| Pipeline | Operations in order over a source, with checkpoints |
| Pipeline step | One entry in a pipeline's `steps`: one operation, called once or once per row. Never called a step alone |
| Plan | The compiled, hashed form of a target version that the engine runs |
| Policy set | The versioned rules the policy engine evaluates |
| Principal | Anyone or anything that calls Anything2MCP |
| Protected setting | A setting that can be changed on one machine only with a user-presence prompt |
| Provider | Anything that supplies capabilities to the runtime: a pack, a plug-in, or an upstream MCP server |
| Probe | For a session: a cheap signed-in read. For a target: the first inspection of an application |
| Profile | A deployment shape, such as Solo or Company server. Also a browser's data directory; context makes clear which |
| Project | A folder with a `anything2mcp.toml` holding target files |
| Quarantine | The wait after a lost lease before the run's records may be touched |
| Read-back | Reading a value from the application after writing it |
| Reconcile | Reading doubtful records back to learn whether their writes landed |
| Record | One item in the application that a run reads or writes, identified by a key. A ledger row is a commit point, which usually carries one record but can carry several, or part of one |
| Review summary | The risk-ordered list a person sees before publishing or importing a version |
| Run | One call of an operation or pipeline, identified by a `run_id` |
| Scenario | A short, declared sequence of named steps that exercises one thing in a target. The unit of observation during discovery |
| Runner | The part that executes steps and holds live sessions |
| Scout | The fixed pipeline that identifies a target, runs its discoverers, drafts what follows by rule, tries the drafted reads, and reports. It runs no model and never tries a write |
| Screen model | A named page or window: how to reach it, its fields, and how to know it is ready |
| Session | The signed-in state of one identity on one target in one environment |
| Session keeper | The only part that signs in, checks, renews, and ends sessions |
| Shadow | A binding that predicts what it would do without acting, to earn promotion |
| Snapshot | A compact view of the current page or window, built from the accessibility tree |
| Source | A reader that turns a file or another system's output into validated records |
| Step | One fixed instruction in a compiled plan, carried out by a driver |
| Step trace | One structured record per step of a run |
| Subject | What a finding or a piece of evidence is about, as one tagged identity: a file by digest, an address, a process, a session, a device, or unknown with a reason |
| Supervisor | The part of the kernel that starts, talks to, and stops outside processes and connections, and stops only what it can prove it started |
| Surface | One way into a target: a web origin, an API base address, an MCP server, a desktop window, a program, a database, a file, a broker, a device link |
| Target | One system being wrapped: an application, a service, a program, a data store, a device |
| Test stand | The piece of test equipment that imitates one kind of system and reports ground truth through a control interface. The test application is the first |
| Tool | An operation or pipeline as an agent sees it |
| Toolset | A named, versioned selection of tools with its own endpoint, members, and policy |
| Trace signature | A hash of the parts of a step trace that should not vary between identical runs |
| Vault | Encrypted storage for sessions, credentials, and evidence |
| Version | An immutable, tested, published snapshot of a target |
| Workspace | An isolation boundary inside one installation |
