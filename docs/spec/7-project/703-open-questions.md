# 703. Decisions and open questions

This section lists the decisions taken in the specification and the questions still open. It is a list, not a design, and describes no code.

Decisions taken in this spec:

| Decision | Chosen | Main alternative | Why |
| --- | --- | --- | --- |
| Name | Anything2MCP | A unique name | Owner's decision; other projects share it |
| Languages | Rust for the core and by default; another language where it clearly fits better | One language for everything | Speed, memory, single-file install, safety; owner's decision |
| AI at run time | Never | Agent fallback when replay fails | Determinism and accountable writes |
| Browser engine | Own Rust engine on the DevTools protocol, Chromium family first | Playwright through a Node sidecar | No extra process; event-driven waits; only a dozen verbs needed |
| Modes | Seven drivers behind one interface | Browser only | Reach and speed; one operation, many bindings |
| Mode choice | Fastest verified binding, by declared rules | Fixed per tool | Speed without losing correctness |
| New bindings | Shadow, then promote with approval | Trust on first success | Catches front-end logic and drift |
| Architecture | Control plane plus runners, combinable in one process | Single process only | One design from laptop to cluster; user-device runners |
| Identity on servers | Four modes, chosen per target and toolset | Service account only | Each user's own rights where possible |
| Tool exposure | Toolsets with their own endpoints; pipelines listed, single operations searched | One flat list | Governance; tool choice degrades past roughly 30 to 50 tools |
| Server protocol | Stateless Streamable HTTP, OAuth resource server | Sessions and custom auth | Matches the current MCP revision and hosted agents |
| Approvals | Outside the agent's reach, bound to the diff | Agent-confirmed | An agent must not approve itself |
| Write channel default | UI until an HTTP binding is proven | Direct HTTP from the start | The app's own validation runs |
| Formats | YAML for targets, TOML for system settings, CEL for expressions | Code-defined tools | Reviewable, lintable, agent-friendly |
| Stores | SQLite locally, Postgres on servers | An external workflow engine | Small footprint; the ledger is a simple state machine |
| Plug-ins | Separate processes over a small protocol | In-process dynamic libraries | Any language; isolation |
| Bot defenses and MFA | Stop and get an allowlist; people complete MFA | Evasion; stored codes | Operates openly |
| License | MIT | Apache-2.0 | The owner's choice: the shortest and most widely understood terms ([record 0057](../../decisions/0057-the-licence-is-mit.md)) |
| Example targets | None named; an imaginary `example` app only | Named real applications | Owner's decision; keeps the project general |
| Package names | `anything2mcp` on pip and npm (not crates.io: [decision 0242](../../decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md)); `a2m` as the short command and the crate prefix | A shorter name everywhere | `a2m` is another project's package on two registries ([record 0056](../../decisions/0056-one-name-everywhere-anything2mcp.md)) |
| Building interface | Commands and files first, MCP tools second, both generated from one registry | MCP tools only | Context cost; agents are strongest with a shell |
| Local process model | One engine per user; clients connect over a socket or named pipe | An engine per MCP client | Shared sessions and locks; no open port |
| pip and npm packages | Ship the binary plus a thin SDK | In-process bindings | Smaller package; no engine logic outside Rust |
| Local approvals | Raised by the engine, with an OS user-presence check where available | An approve command | An agent with a shell must not approve itself |
| Ledger shape | One row per commit point in the application; append-only events beside current state | A step log only | Exact resume; rebuildable history |
| Runner link | HTTPS requests and long polls, mutually authenticated; the same interface in memory | A streaming connection | Passes proxies; stateless front; one test suite |
| Lost leases | Runner stops early, records quarantined, reconciled, verified again later | Reassign at once | The application cannot check lease numbers |
| Who may use a user's session | Only that user and agents acting for them; no administrator override | Any caller with a grant | Owner's requirement; each person's account stays theirs |
| Browser profiles | Always the engine's own; never the user's everyday profile | Attach to the user's browser | Chrome blocks it; it is what cookie stealers do |
| Session sharing | Runs share one live session in separate tabs; one renewer | A copied context per run | Copies go stale; device-bound sessions cannot be copied |
| Policy decisions | The policy crate's own evaluator: deny by default, forbid wins; grants are permits ([decision 0082](../../decisions/0082-the-decision-core-is-the-policy-crates-own-evaluator.md)) | Cedar | Its tree fails the closed licence list and adds 107 crates and 5.5 MB |
| Policy lifecycle | Versioned, tested, immutable; every decision recorded and explainable | Live-edited settings | A policy change is as risky as a code change |
| Target files at run time | Compiled once into a hashed compiled target; never parsed during a call | Interpret YAML on each call | Speed, early errors, a hash for exactly what ran |
| Evidence | Events, a step trace, and artifacts; redacted at capture; a trace signature for comparing runs | Logs | Gates, repair, and audit all consume it |
| Keys | Root, workspace, owner, and data keys; root in the OS store or a key service | One key for everything | Rotation without re-encrypting; ownership backed by encryption |
| Local approval surface | A diff page served for that moment, then an operating-system prompt; no tray application at first | A system dialog alone | A diff needs a page; a page alone can be approved by a script |
| Outbound data | Nothing leaves the machine unless switched on | Usage statistics by default | The tool holds live sessions to business systems |
| Finding a record again | Five identity strategies; assigned keys and results kept in the ledger | Class every create irreversible | Creates and header-with-lines pipelines must be resumable |
| A manipulated agent | Typed templates, allowed file roots, a risk-ordered publish review, protected settings | Trust the agent | Agents read text that can steer them |
| Engine faults | No ledger, no write; nothing unverified runs; three health states | Best effort | Fail closed, and say why |
| Version rollout | Published, then candidate with shadowed reads, then current | Switch on publish | A bad version is the likeliest self-inflicted failure |
| Secrets on runners | User-device sessions never leave the device; server runners get wrapped material per job | Control plane holds everything | Limits what any one compromise exposes |

Open questions:

- [ ] For each real target: what does it send when a record is saved (JSON to an API, form posts, or neither)?
- [ ] For each real target: do IT and the vendor's terms allow wrapping it, and with which identity mode?
- [ ] For each real target: is there a test environment or a test record that tests may write to and restore?
- [ ] Have the package names been reserved on PyPI and npm?
- [ ] Which user-presence checks can a background process call on each operating system?
- [ ] Is long polling fast enough for interactive reads, or is a streaming carrier needed behind the same interface?
- [ ] Who issues runner certificates: a built-in authority or the organization's own?
- [ ] Does a fresh engine-owned profile sign in silently through the device's account on each operating system?
- [x] Do Cedar's size and decision time fit the budgets? The time does; the size and the tree's licences do not (decision 0082).
- [ ] Which base for the browser engine: chromiumoxide, Rustwright, or a new protocol client?
- [ ] Which MCP revision and features does Copilot Studio support today, and which network bridge will the company use?
- [ ] Can an Entra app registration be granted delegated permission to any target's API, enabling `per_user_token`?
- [ ] Who approves write runs, and through which channel?
- [x] Are the package names free on crates.io, pip, and npm? `anything2mcp` was free on all three on 2026-11-17, and is held on none until a release is published ([publishing setup](../../plan/publishing-setup.md)).
- [ ] How long must traces and audit records be kept?

Research still to do before the matching phase: durable-execution patterns for the ledger; session lifetime and device rules under Entra; vault design per operating system; remote sign-in to a hosted browser; selector repair; enterprise approval patterns; prompt-injection defenses; testing against third-party apps; desktop automation on servers; lessons from RPA products.
