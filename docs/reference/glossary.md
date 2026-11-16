# Glossary

One meaning per word. Code, files, messages, and documents use these terms and no synonyms.

| Term | Meaning |
| --- | --- |
| Action | An authoring or administration function defined once in the action registry and exposed as a command, an MCP tool, a REST route, and an SDK method |
| Approval | A person's decision to allow one exact diff, stored as a single-use record |
| Artifact | A large piece of evidence: a snapshot, a network exchange, a screenshot |
| Binding | How one mode carries out one operation. An operation can have several |
| Candidate | A published version being compared in shadow before it becomes current |
| Blob | Any stored evidence or value, named by its content hash and encrypted |
| Client | Anything that talks to the engine: a command, an MCP client, an SDK, the tray application |
| Commit point | One save in the application. One ledger row |
| Control plane | The part that decides, records, and hands out work |
| Diff | Current values against proposed values, record by record, shown before a write |
| Diff hash | A hash of a diff in canonical form; what an approval is bound to |
| Draft | Target files not yet published; runnable only in the sandbox |
| Driver | The code that carries out steps in one mode |
| Effect class | `read`, `write`, or `irreversible` |
| Engine | The Web2MCP process. On one machine, one per user |
| Environment | A named instance of a target, such as test or production |
| Evidence | Everything recorded about a run: events, step trace, artifacts |
| Failure class | The category of a stop, which alone decides what happens next |
| Fixture | Scrubbed recorded evidence used for offline tests |
| Grant | A rule giving a principal access to a scope up to an effect class |
| Home runner | The runner holding a session's live state |
| Health state | `ok`, `degraded`, or `stopped`: what the engine can currently do |
| Identity | Who the target application sees |
| Identity strategy | How a write's record is found again: natural, composite, stamped, find, or none |
| Identity mode | How that identity is obtained: per-user token, per-user session, per-user device, or service account |
| Intent | A ledger entry made before a write, recording what is about to be sent |
| Job | A piece of a run handed to a runner under a lease |
| Lease | Time-limited permission for one runner to work on one job, with a number that only increases |
| Local channel | The per-user socket or named pipe between clients and the engine on one machine |
| Mode | One of the seven ways to reach an application: `ui`, `page_http`, `http`, `api`, `site_tools`, `mcp`, `desktop` |
| Operation | One business action with typed inputs and outputs, defined once |
| Owner | The person a user session belongs to |
| Pipeline | Operations in order over a source, with checkpoints |
| Plan | The compiled, hashed form of a target version that the engine runs |
| Policy set | The versioned rules the policy engine evaluates |
| Principal | Anyone or anything that calls Web2MCP |
| Protected setting | A setting that can be changed on one machine only with a user-presence prompt |
| Probe | For a session: a cheap signed-in read. For a target: the first inspection of an application |
| Profile | A deployment shape, such as Solo or Company server. Also a browser's data directory; context makes clear which |
| Project | A folder with a `web2mcp.toml` holding target files |
| Quarantine | The wait after a lost lease before the run's records may be touched |
| Read-back | Reading a value from the application after writing it |
| Reconcile | Reading doubtful records back to learn whether their writes landed |
| Record | One item in the application that a run reads or writes, identified by a key |
| Review summary | The risk-ordered list a person sees before publishing or importing a version |
| Run | One call of an operation or pipeline, identified by a `run_id` |
| Runner | The part that executes steps and holds live sessions |
| Screen model | A named page or window: how to reach it, its fields, and how to know it is ready |
| Session | The signed-in state of one identity on one target in one environment |
| Session keeper | The only part that signs in, checks, renews, and ends sessions |
| Shadow | A binding that predicts what it would do without acting, to earn promotion |
| Snapshot | A compact view of the current page or window, built from the accessibility tree |
| Source | A reader that turns a file or another system's output into validated records |
| Step | One fixed instruction in a compiled plan, carried out by a driver |
| Step trace | One structured record per step of a run |
| Surface | One way into a target: a web origin, an API base address, an MCP server, a desktop window |
| Target | One application being wrapped |
| Tool | An operation or pipeline as an agent sees it |
| Toolset | A named, versioned selection of tools with its own endpoint, members, and policy |
| Trace signature | A hash of the parts of a step trace that should not vary between identical runs |
| Vault | Encrypted storage for sessions, credentials, and evidence |
| Version | An immutable, tested, published snapshot of a target |
| Workspace | An isolation boundary inside one installation |
