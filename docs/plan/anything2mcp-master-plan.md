# Anything2MCP: the long-term plan

Written on 2026-11-16, after the owner decided to turn Web2MCP into Anything2MCP. This is the one document that holds the whole direction: what exists today, what was learned from the REA project, the target architecture, the names, the order of work, and what is still the owner's to decide. A lead agent and the agents working under it should be able to plan from this document alone and open the other documents only for detail.

Nothing in this document has been built. It is an audit and a design. Where it proposes, it says so.

**Since 2026-11-17** the architecture this plan proposes is written out as spec sections 53 to 66 ([index](../spec/README.md)), with decision records 0049 to 0054 and a fuller study of REA in [research note 16](../research/16-rea-methods.md). Where those sections and this plan differ, the sections win. This plan remains the place for the audit, the names, the order of work, and the questions for the owner.

**Since 2026-11-17, second session,** the owner has answered the questions in section 9, and Stage A is done apart from package A7. What changed:

| Was | Now | Record |
| --- | --- | --- |
| Agents wait for the owner's go-ahead | Agents plan, build, and merge on their own. Section 8, rule 7 is replaced by AGENTS.md, "What an agent does not do alone" | [0055](../decisions/0055-agents-plan-build-and-merge-without-waiting-for-the-owner.md) |
| Names proposed in section 5, with the earlier names kept as alias packages | The names are decided. There are no alias packages | [0056](../decisions/0056-one-name-everywhere-anything2mcp.md) |
| Apache-2.0 | MIT | [0057](../decisions/0057-the-licence-is-mit.md) |
| Audience, catalog, clients, first real system: open | A toolkit for developers; no public catalog; no preferred agent; test stands throughout and real systems at the end | [0058](../decisions/0058-a-toolkit-for-developers-with-no-catalog-and-no-preferred-agent.md) |
| An agent or a person writes every draft | A scout drafts what follows by rule and proves reads; spec section 67 | [0059](../decisions/0059-the-scout-drafts-and-proves-reads-and-three-things-stay-with-the-developer.md) |
| Reverse engineering: how far? | In scope, automated where it can be, inside the limits already decided | 0058 |

Sections 2.4, 5, and 9 and appendix A below describe the rename as something still to do, and are kept as the record of what was planned. The marks **Proposed** and **Open** on anything in that table no longer apply.

## 0. How to read this

Every statement of direction carries one of three marks.

| Mark | Meaning |
| --- | --- |
| **Decided** | The owner said it. Build on it |
| **Proposed** | The design this document recommends. Build on it once the owner has not objected, or a test supports it; record the choice as a decision record when it is first built on |
| **Open** | Needs the owner, or needs evidence. Do not build on a guess |

The existing documents stay in force wherever this one is silent. [AGENTS.md](../../AGENTS.md) still governs how work is done, the [working method](working-method.md) still governs each slice, and the ten rules that never bend are unchanged. Section 8 lists what this document changes in them.

Work is expected to be done by a lead agent that plans and reviews, with deep-work agents for design-heavy or risky packages and fast agents for mechanical ones. Section 7 marks each work package with the kind of agent that suits it.

## 1. The direction

**Decided.**

- The product is **Anything2MCP**. Web2MCP was the first shape of it.
- One line: *turn software and systems into safe, deterministic, agent-accessible capabilities.*
- Rust-powered core. Python-first distribution: `pip install anything2mcp`. MCP-native interfaces.
- It is a universal runtime, not a converter with one purpose. It covers four jobs:

| Job | What it includes |
| --- | --- |
| Discover and understand | Introspection, capture, analysis of how a system works, finding what it can do |
| Integrate and execute | Really carrying out operations, not only writing tool definitions |
| Verify | Proving that an operation does what it claims, and keeping the proof |
| Expose | MCP tools first, and also commands, a Python interface, and a REST interface |

- It is modular and highly customizable. A person who needs one kind of integration does not carry the rest.
- Anything2MCP owns its abstractions. Other projects, REA among them, supply capabilities through those abstractions and never define them.
- The system already designed is kept and extended. It is not restarted.

**What does not change.** The ten rules in AGENTS.md: no AI at run time; safe, then correct, then fast, then easy; every write verified; secrets never reach a caller; operate openly; Rust first; unknown means stop; knowledge in one place; no real application named; no test writes against a real application. The engine loop, the ledger, the policy engine, the compiler, the action registry, the local engine, and approvals out of the agent's reach all stand as designed.

**What changes.** The name. The boundary between a neutral kernel and the parts that know about one kind of system. A provider contract that lets capabilities come from several sources. Discovery as a first-class job with its own evidence levels. Python-first packaging with optional parts.

## 2. Audit: what exists today

### 2.1 Built and proven

| Area | What exists | State |
| --- | --- | --- |
| Workspace | One Cargo workspace, 23 crates named after spec section 29, all empty; Rust 1.100.0 pinned; lints deny `unsafe`, `unwrap`, `expect` | Builds and tests on Windows, macOS, Linux in CI |
| Binary | `web2mcp` reports its version, refuses anything else with exit code 2 | 4 tests |
| Test application | `targets/example-app/`: an imaginary order-entry system; cases 1 to 4; a control interface that reports what was stored; a harness that tests use to ask it | 18 tests, 12 over real HTTP |
| Audit script | `tools/docs_audit.py`: links, numbering, names, failure classes, states, commands, replaced phrases, counts, crate layering, one version, one licence text | 13 planted mistakes prove it fails when it should |
| CI | Format, lint, build, test on three systems; documents and layering; licences and advisories against a closed list | Green on `main` |
| Release | `release.yml` builds and checks a placeholder pip package; publishes only on a version tag | Build half proven; nothing published |

### 2.2 Designed and not built

52 spec sections, 48 decision records, 8 reference documents, 15 research notes, 31 open research briefs, 28 hypotheses (all open), task lists for Phases 0 to 3, and the slice order in [core first](core-first.md). The whole engine is design: no loop, no driver, no ledger, no policy, no compiler exists as code.

Slices left in Phase 0: the publish that finishes 0.2; 0.4, the browser engine spike; 0.5, the MCP server spike; 0.6, core types and file schemas.

### 2.3 How much of the design is already universal

More than the old name suggests. This matters, because it means the refactor is a separation and a renaming, not a redesign.

| Already neutral | Where |
| --- | --- |
| Seven modes behind one driver interface, with the engine, ledger, and policy the same in every mode | Spec sections 6 and 8; decision 0005 |
| A mode that re-exposes any existing MCP server with policy, approval, and audit around it | The `mcp` mode |
| Plug-ins as separate processes in any language, for drivers, auth providers, source readers, widget drivers | Decision 0016; the plug-in protocol in the technical reference |
| Extension drivers named in the design: a command-line driver, a database driver | Spec section 6 |
| One registry from which every command, MCP tool, REST route, and SDK method is generated | Spec section 40; decision 0023 |
| The engine loop, ledger, identity strategies, policy, compiler, expressions, field types, sources, vault, traces, approvals, self-failure rules | Spec sections 8, 10, 12, 18, 42, 45 to 52 |

| Specific to web and desktop applications | Where |
| --- | --- |
| Screen models, locators, fingerprints, widget drivers, platform profiles, snapshots | Spec sections 13, 14, 17; file formats |
| The browser engine, in-page HTTP, site tools | Spec sections 6 and 17 |
| Sessions as a signed-in browser profile; the sign-in window | Spec sections 15, 44, 49 |
| Capture and endpoint mining from a browser | Spec section 26 |

The split in section 4 follows this table.

### 2.4 What the old name is attached to

| Where | Size |
| --- | --- |
| Files that mention the product name | 111 files, 341 mentions |
| The crate prefix and the built-in tool prefix | 217 mentions in 51 files |
| Rules in the audit script that encode names | The crate check, the built-in tool check, the command check |
| Package names | `web2mcp` held on crates.io and npm (`w2mcp`); not held on PyPI |
| Decision records | 0001 (the name) and 0022 (package names) |

### 2.5 Debts carried into the new plan

- The PyPI name is not held. See section 9, question 2.
- AGENTS.md and three skills still tell an agent to wait for a go-ahead and not to push unasked, while the owner has said the agent merges its own work. Section 9, question 4.
- Open points written into spec sections 42 and 47, the file format reference, and the technical reference.
- Hypotheses H-23 (an upsert in one `ui` binding), H-24 (record keys without a record kind), H-25 (the crate layering holds).
- Unicode-3.0 is on the allowed licence list without the owner having been asked. `version_conflict` was moved under `stale`.

### 2.6 Two facts about the environment

- From the build session, the repository still answers as `MHughesDev/web2mcp`, and no repository named `anything2mcp` is visible to it. Either the rename has not taken effect or the session cannot see it. Nothing in the repository has been changed to a new address. Section 9, question 1.
- A build session can push only its own branch. It cannot push a tag, so it cannot start a publish. The owner pushes a release tag, or the release workflow gains a manual start that an agent may trigger. Work package F2.

## 3. REA: what it is and what to take from it

REA was read at commit `8ffea11` of <https://github.com/morluto/rea> on 2026-11-16. Its source was inspected; nothing in it was run.

### 3.1 Facts

| | |
| --- | --- |
| What it is | "Reverse Engineer Anything": one command-line tool and one MCP server for understanding software without its source |
| Package | `rea-agents` on npm, version 6.9.0; started as an MCP server with `npx rea-agents mcp`; every tool is also a command with `--json` |
| Licence | MIT for REA itself |
| Language and runtime | TypeScript on Node.js 22 or newer. Bridges in Java, Python, and Swift; a small Windows add-on in C++ |
| Size | About 9,100 files: 4,400 source, 3,000 tests, 500 documents |
| Tools | On the order of 190 |
| External engines | Hopper, Ghidra, IDA, Kuna, JEB, Cutter and rizin, objdump, gdb with pwndbg, JADX, apktool, adb, binwalk, unblob, Frida, mitmproxy, Playwright. None is bundled; the user supplies each, and REA never downloads one during an operation |

What its tools cover, by family:

| Family | Examples of what it does |
| --- | --- |
| Web applications | Capture a browser session and its network traffic; inspect a page and its event listeners; analyze a JavaScript bundle statically; trace imports; compare endpoints found in code with endpoints seen in traffic; read an offline API contract; discover tools a site publishes for agents |
| JavaScript and Electron | A static analysis engine of its own, which does not execute the target's code; Electron boundaries and fuses; runtime observation |
| Desktop applications | Inspect an application's structure and, on macOS, its interface through the accessibility system |
| Mobile | Android packages, classes, resources, signatures; device control through adb |
| Native binaries | Decompilation and search through whichever engine the user has; binary layout, imports, hardening; debugging and instrumentation |
| Managed and other runtimes | .NET, IL2CPP, Flutter, Go, Rust, Erlang, Python bytecode, WebAssembly, EVM contracts |
| Artifacts and data | SQLite databases, property lists, archives, packet captures, firmware images, kernel modules, GPU code objects, model artifacts |
| Its own discipline | Evidence records, target identity, declared effects per tool, reconstruction ledgers, audits of what remains unknown |

How it is built, which matters more than the list:

- **Layers with a checked boundary.** A pure domain layer, contracts, application services, ports, provider adapters, composition, and two faces (command line and MCP). A script fails the build when an import crosses a boundary.
- **Provider adapters.** Each outside engine sits behind an adapter. Upstream source is never copied into REA's own code. An engine is identified by its SHA-256 and reported as unknown when it does not match an audited release.
- **Evidence.** Every result carries an Evidence record: what was observed, about which subject, with which limitations. A decision record of theirs notes that 50 of 120 places could not name their subject, and fixes it with one tagged target identity for a file, a URL, a process, or a session.
- **Declared effects.** Each tool declares whether it launches a process, writes to the filesystem, uses the network, or holds a session.
- **Claims are checked.** Tools exist to build a ledger of what a reconstruction must still prove, to verify a reconstruction, and to audit residual unknowns. An inferred fact is never presented as an observed one.
- **Context economy.** Compact input schemas, output schemas left out of the tool list by default, and a tool for discovering tools.

### 3.2 What REA is not

It does not carry out business operations. It has no ledger, no read-back of a write, no approval of a diff, no resume without duplicates, no policy engine, no kept sign-in. It answers "how does this work?", with evidence. Anything2MCP answers "do this, safely, and prove it was done". The two meet at one place only: what REA learns can feed what Anything2MCP builds.

That meeting place is at build time. The first rule, no AI at run time, already draws the line: discovery and drafting may use any help; a run uses none. REA belongs on the build side of that line and never on the run side.

### 3.3 Verdict, capability by capability

**Proposed.**

| REA capability | Verdict | How |
| --- | --- | --- |
| The whole of REA as a required part | **No** | It needs Node.js 22, is very large, and most of it is unrelated to integrating a system |
| The whole of REA wrapped as one giant tool list | **No** | About 190 tools would bury an agent's tool list; the design already limits the list to under 20 |
| REA as an optional discovery provider | **Yes, optional** | Attach it as an upstream MCP server through the existing `mcp` mode, with only chosen tools mapped to discovery actions. No REA code enters this repository. Work package E3 |
| Web capture, bundle analysis, and "endpoints in code against endpoints in traffic" | **Take the idea; build our own for the common path** | Endpoint mining is already planned in Rust. The comparison of static and observed endpoints is the idea to adopt: it is exactly the line between inferred and discovered in section 4.5. REA can add static bundle analysis on request |
| SQLite, packet capture, property lists, archives | **Optional, through REA** | Useful for understanding an application's data. Not kernel |
| Desktop accessibility inspection | **Our own** | The desktop pack is already designed around accessibility systems on three operating systems. REA covers macOS only |
| Discovery of tools a site publishes for agents | **Our own** | The `site_tools` mode already covers it |
| Native binaries, debugging, instrumentation, firmware, kernel, contracts, GPU and model artifacts | **Optional, through REA, off by default** | Available to a person who needs them, never installed or listed otherwise. See 3.5 |
| Evidence records with a tagged target identity | **Adopt the idea** | Section 4.5 |
| Declared effects per tool | **Adopt the idea** | It extends the permission manifest already in the design to providers. Section 4.3 |
| Engines identified by digest, never fetched during an operation | **Adopt the idea** | Section 4.9, managed components |
| A boundary check between layers | **Already have it** | The audit script enforces the crate layering |
| Ledgers of what remains unproven | **Adopt the idea** | It matches "unknown means stop" and the path from draft to verified |
| Compact schemas and tool search | **Already designed** | Decision 0010 |

### 3.4 How the optional REA provider works

**Proposed.**

1. A person switches it on. The runtime checks for Node.js and for `rea-agents`, and says exactly what is missing and how to get it. It installs nothing by itself.
2. The runtime starts REA as a subprocess speaking MCP over standard input and output, pinned to one version.
3. A small mapping file, in this repository, names the REA tools that are exposed and the discovery action each one serves. Everything else stays unreachable.
4. What comes back is stored as findings with an evidence level and the limitations REA reported. A finding is never an operation.
5. The provider can only read. The engine enforces that exploration cannot write, as it already does for its own exploration.

### 3.5 Limits on reverse engineering, stated plainly

**Proposed, and partly Open.**

- Rule five, operate openly, is unchanged. Nothing learned by inspection may be used to evade a bot defense, bypass sign-in or MFA, imitate another application's client identity, or defeat a licence check or copy protection.
- Inspecting an application one does not own can be restricted by its terms or by law. The existing requirement of an authorization record before a target is published on a server stays, and applies equally to anything discovered through REA.
- Instrumentation and debugging of a running program are off unless a person switches them on for one target.
- Open: whether the owner wants native-code capabilities offered at all in the first releases. Section 9, question 6.

## 4. Target architecture

**Proposed** throughout, except where marked.

### 4.1 The shape

```text
 Agents and programs
   Claude Code, Codex, other MCP clients, Python programs, people at a terminal
        |
        |  MCP  ·  commands  ·  Python  ·  REST            (all generated from one registry)
        v
 +---------------------------------------------------------------------------+
 |  KERNEL (knows nothing about any one kind of system)                       |
 |                                                                           |
 |  actions registry · local engine · front doors                            |
 |  compiler and plan · expressions · field types · sources                  |
 |  engine loop · ledger · policy and approvals · sessions · vault · traces  |
 |  provider registry and plug-in host · evidence store                      |
 +---------------------------------------------------------------------------+
        |  provider contract: one manifest, a fixed set of extension points
        v
 +------------------+ +-------------+ +-------------+ +--------------+ +-----------+
 | web pack         | | http pack   | | mcp pack    | | desktop pack | | later:    |
 | browser, ui,     | | http, api,  | | upstream    | | accessibility| | cli, sql, |
 | page_http, site  | | descriptions| | MCP servers | | on three     | | files ... |
 | tools, capture   | |             | | (REA here)  | | systems      | |           |
 +------------------+ +-------------+ +-------------+ +--------------+ +-----------+
        |                                   |
        v                                   v
   real systems                    outside providers as processes:
                                   plug-ins in any language, upstream MCP servers
```

### 4.2 Kernel and packs

The kernel is everything that is the same whatever is being integrated. A pack is everything that knows about one kind of system. The rule that keeps them apart is one line, and the audit script enforces it: **a kernel crate never depends on a pack crate; a pack depends only on the kernel; the product crate wires them together.**

| Kernel | Pack |
| --- | --- |
| Core types, files and compiler, expressions | Web: the browser engine, the `ui` and `page_http` drivers, widget drivers, platform profiles, site tools, capture and endpoint inference |
| The engine loop, ledger, policy, scheduler | HTTP: the `http` and `api` drivers, import of API descriptions |
| Sessions and the vault | MCP: upstream MCP servers |
| Sources, the action registry, the local engine | Desktop: accessibility on three operating systems |
| Front doors: MCP server, REST, runner link | Later, each a pack of its own: command-line tools, databases, files, and whatever comes after |
| Traces, the provider registry, the plug-in host | |

The web pack is what Web2MCP was. It stays the first and deepest pack, and the core-first slices continue to build the kernel through it.

**A pack is not a second-class citizen and not a plug-in.** First-party packs are Rust crates compiled into the one binary. They are switched on by a setting, not by an install, because the binary without a browser is small. Section 4.9.

**What a new kind of system must bring.** A pack for a new surface kind is accepted only with all of these, so that "anything" never comes to mean "unverified":

1. A driver that implements the driver interface for its verbs.
2. A read-back rule for every write it offers, or the write is `irreversible`.
3. An answer for each identity strategy: how a record is found again.
4. A case in the test application family that imitates it, with a control interface that reports ground truth.
5. A discoverer, if the kind can be inspected.
6. Its failure classes mapped to the existing ones. No new class without a decision record.

### 4.3 The provider contract

A provider is anything that supplies capabilities to the runtime. There are three ways to be one, and they share one manifest.

| Way | What it is | Trust |
| --- | --- | --- |
| A first-party pack | A Rust crate in this repository | Reviewed with the product |
| A plug-in | A separate process in any language, speaking the plug-in protocol already designed | Unsigned ones are off by default; each declares what it needs and an administrator approves |
| An upstream MCP server | An existing MCP server reached through the `mcp` mode | Only mapped tools are reachable; policy, approval, and audit wrap every call |

**The manifest.** One declaration per provider, checked when it is loaded.

| Field | Meaning |
| --- | --- |
| Identity | Name, version, publisher, digest |
| Extension points | Which of the fixed points below it fills |
| Surface kinds and verbs | What it can reach and what it can do there |
| Effects | Launches a process; reads or writes the filesystem, and where; uses the network, and to which destinations; holds a session |
| Requirements | Outside programs or runtimes it needs, how to detect each, and what to tell a person who lacks one |
| Platforms | Which operating systems it supports |
| Health | A cheap check the runtime can run |
| Evidence | The highest evidence level its findings may claim |

**The extension points are a fixed, short list.** Driver, auth provider, source reader, widget driver, and one new one: **discoverer**, which inspects a target and returns findings. A provider cannot add a new kind of extension point; that takes a decision record. This is what "Anything2MCP owns the abstraction" means in practice.

**Lifecycle.** Found, checked against its manifest, enabled by a setting, started when first needed, health-checked, stopped when idle. A provider that asks for more than its manifest declared is stopped, and the failure is named.

### 4.4 Two things an agent uses

The pasted vision asks that the runtime be useful even when no MCP tool is being generated. The design already has the two halves; they only need naming.

| | Actions | Tools |
| --- | --- | --- |
| What | The toolkit: discover, inspect, capture, draft, try, test, publish, administer | Published operations and pipelines on a target |
| Defined in | The action registry, once each | Target files, compiled into a plan |
| Reached by | Command, MCP tool, REST route, Python method, all generated | The same four faces |
| May use AI | Around them, at build time | Never, at run time |
| Come from | The kernel and providers | A person publishing a tested version |

A provider contributes actions through the registry, under its own name space, and never by adding a face by hand. An agent asks one action which capabilities are installed, which are enabled, and what each missing one needs.

### 4.5 The evidence ladder

Discovering that something might work is not proving that it works. Every finding, draft, and tool carries one level, and nothing can be shown at a higher level than its evidence supports.

| Level | Meaning | What proves it |
| --- | --- | --- |
| `inferred` | Derived, never observed: from static analysis, a pattern, or a guess | Nothing yet; the source of the inference is recorded |
| `discovered` | Observed directly: seen in a capture, listed in a description, present on a screen | The captured evidence, with the identity of what was observed |
| `reconstructed` | A draft operation exists, with inputs, outputs, and bindings | The draft compiles and lints |
| `schema_verified` | Real responses matched the declared schemas | Fixtures and a schema check |
| `executed` | The draft ran in the sandbox against a test record | A step trace |
| `behavior_verified` | The publish gates passed: round trip, idempotency, resume, and ground truth where the test application can give it | Gate evidence stored with the version |
| `approved` | A person published it and it is current | The publish record |

How this meets what is already designed: binding statuses stay `draft`, `shadow`, `verified`, `retired`, and version stages stay published, candidate, current. The ladder is added to findings and drafts, and the compiler refuses a binding marked `verified` whose version has not reached `behavior_verified`. Only `approved` tools are listed to ordinary callers. Each piece of evidence names its subject with one tagged identity: a file by digest, an address, a process, or a session, with the version of the application where it is known. That last point is taken from REA.

### 4.6 Sessions, generalized

A session is the signed-in state of one identity on one target in one environment. Today's design describes it as a browser profile or a token set. For other kinds of system it is a database connection's credentials, an API key, a command-line tool's own stored login. The session keeper stays the only part that signs in or renews, the vault stays the only place a secret rests, and each pack supplies an auth provider. Nothing else changes. The rule that a person's session is used only by that person or an agent acting for them holds for every kind.

### 4.7 What an agent experiences

```text
pip install anything2mcp
anything2mcp init                     # a project folder and the guidance file
anything2mcp providers list           # what is installed, enabled, or missing, and what to do about it
anything2mcp add <address or path>    # register a target; the probe picks discoverers by what it finds
anything2mcp discover                 # findings, each with an evidence level
anything2mcp lint | try | test        # drafts become verified
anything2mcp publish                  # a person reviews; never an agent
anything2mcp connect <client>         # the agent's tool list now has the control tools
```

Every command returns one JSON object and names the next action, as the walkthrough already requires. A short command name, `a2m`, is proposed as an alias. From Python, the same actions are methods on a client that talks to the local engine; no engine logic lives in Python.

### 4.8 Security and who may execute what

Everything in spec sections 18, 19, 45, 48, 49, 51, and 52 stands. Providers add four rules.

1. **A provider gets only what its manifest declared and an administrator approved.** Process launch, filesystem roots, and network destinations are checked by the runtime, not trusted.
2. **Discovery cannot write.** Enforced in the engine for every discoverer, first-party or not.
3. **Nothing is fetched during a run.** An outside program is installed by a person or by an explicit install action, identified by digest, and reported as unverified when the digest is unknown.
4. **Being able to run a command is not permission to run it.** A command-line pack exposes declared commands with typed arguments, never a shell.

Secrets reach a provider only through the injection path already designed, and never in a result, a log, or a trace. On one machine the limit stated in spec section 39 still holds: these protections guard against mistakes, runaway agents, and ordinary prompt injection, not against a hostile program already running as the user.

### 4.9 Packaging and dependencies

| Layer | What it carries | How it is installed |
| --- | --- | --- |
| The base | The one Rust binary with every first-party pack compiled in and switched off until enabled; a thin Python client | `pip install anything2mcp` |
| Outside requirements of a pack | A browser for the web pack; nothing for HTTP | Used where already installed; fetched only by an explicit action |
| Separate provider packages | Plug-ins published by this project or others, found through a Python entry point | `pip install anything2mcp[name]` or the package itself |
| Managed components | Outside programs such as REA's runtime | Detected, never bundled; the runtime says what is missing |

Why first-party packs are compiled in instead of installed separately: the browser is not bundled, so the binary stays small, and one binary means one version, one audit, and no missing-piece failures for the common cases. The target is a wheel under 25 MB, which is hypothesis H-28. What really is heavy or rare, such as reverse-engineering engines, stays outside as a provider.

The name `web2mcp` is kept as a second way in: a small package that installs Anything2MCP with the web pack enabled. The names already held on crates.io and npm keep their purpose. **Open**, section 9, question 2.

## 5. Names

**Decided** on 2026-11-17 by [decision 0056](../decisions/0056-one-name-everywhere-anything2mcp.md), with one change from the table below: no alias packages are kept under the earlier names.

| Thing | Today | Proposed |
| --- | --- | --- |
| Product | Web2MCP | Anything2MCP |
| Pip package | `web2mcp` (not held) | `anything2mcp`; `web2mcp` kept as the web-pack alias |
| Crate on crates.io | `web2mcp` (held) | `anything2mcp`; `web2mcp` kept |
| npm package | `w2mcp` (held) | `anything2mcp`; `w2mcp` kept |
| Binary | `web2mcp` | `anything2mcp`, with `a2m` as a short alias |
| Kernel crates | `w2m-core`, `w2m-engine`, and so on | `a2m-core`, `a2m-engine`, and so on |
| Pack crates | `w2m-browser`, `w2m-driver-ui`, `w2m-probe`, `w2m-driver-http`, `w2m-driver-mcp`, `w2m-driver-desktop` | `a2m-pack-web-browser`, `a2m-pack-web-ui`, `a2m-pack-web-probe`, `a2m-pack-http`, `a2m-pack-mcp`, `a2m-pack-desktop` |
| New kernel crate | None | `a2m-provider`: the manifest, the registry, the plug-in host |
| Built-in tool prefix | `w2m_` | `a2m_` |
| Project file | `web2mcp.toml` | `anything2mcp.toml` |
| Working folder | `.web2mcp/` | `.anything2mcp/` |
| Home variable | `W2M_HOME` | `A2M_HOME` |

Registry checks on 2026-11-16: `anything2mcp` is free on PyPI, crates.io, and npm. `a2m` is taken on PyPI and npm, so it can be a command name and a crate prefix and never a package name. `any2mcp` is free on all three and is worth holding as a spelling people will try.

## 6. Trade-offs, and why this way

| Choice | Instead of | Why |
| --- | --- | --- |
| One repository, kernel and packs in one workspace | A second repository for the universal product that swallows the first | The owner's later decision. One version, one audit, one CI. The dependency rule gives the separation that two repositories would, without two release trains |
| Rename in place | Leaving Web2MCP as it is and starting Anything2MCP beside it | Nothing of the engine is built yet, so renaming now costs a few hundred mechanical edits. Later it would cost a migration |
| REA as an optional provider behind the `mcp` mode | Vendoring its code, or porting its tools | Its value is the engines behind it and its discipline, not code we could reuse in a Rust core. The `mcp` mode exists for exactly this |
| A fixed list of extension points | An open plug-in system where a provider can add any kind of thing | Every extension point is a public contract that the engine, policy, and ledger must understand. An open list would make "verified" mean less |
| First-party packs compiled in | A package per pack | Small binary, one version, fewer ways to be half-installed |
| The evidence ladder as data on findings and drafts | A separate catalog of "possible tools" | One compiler and one set of gates already decide what may run. A second catalog would be a second source of truth |
| Rust core with Python as the way in | A Python core | Rule six, and the speed and safety budgets the design is built on. Python stays the thinnest possible client |
| Prove the kernel through the web pack first | Building several packs at once | The core-first method. A second pack is added early on purpose, to test the boundary, and no more than one |

## 7. The order of work

Stages are lettered so that they are not confused with the build phases 0 to 9, which keep their numbers and their meaning. Within a stage, packages marked parallel can run at once on separate branches.

**Agent kinds.** *Lead*: plans, reviews, merges, talks to the owner. *Deep*: design-heavy or risky work. *Fast*: mechanical work with a clear check.

### Stage 0: the owner's answers

Done on 2026-11-17. The answers are in the table at the top of this document.

### Stage A: rename and separate

Mechanical, and proven by the audit script and CI. No behavior changes. **Packages A1 to A6 were done on 2026-11-17**, with the manual start of package F2 brought forward. A6 was done without an alias package. **A7 remains.**

| # | Package | Depends on | Proof | Agent | Parallel |
| --- | --- | --- | --- | --- | --- |
| A1 | Decision records: the names in section 5; the kernel and pack rule; supersede 0022 | Stage 0 | The audit passes | Lead | No |
| A2 | Rename the crates and move pack crates under their new names; update `Cargo.toml` files and the layout in spec section 29 and the layering table | A1 | Build and tests green on three systems | Fast | No |
| A3 | Teach the audit script the new names and the rule that no kernel crate depends on a pack crate, with planted mistakes | A2 | The self-test fails for each planted mistake | Deep | With A4 |
| A4 | Rename the binary, the built-in tool prefix, commands, project file, working folder, and home variable across code and documents | A2 | The audit's name checks pass; the binary's tests pass under the new name | Fast | With A3 |
| A5 | Rewrite the product name in every document, README, AGENTS.md, and skill; keep decision records' history intact by superseding, never rewriting | A4 | No mention of the old name outside history, the alias package, and decision records | Fast | With A6 |
| A6 | Rename the pip placeholder to the new name and add the `web2mcp` alias package; repoint the release workflow | A1, question 2 | The release workflow's build job is green for both | Fast | With A5 |
| A7 | Classify every spec section as kernel or pack, in the spec index, and move the handful of sentences that mix the two | A5 | A reader can tell from the index which pack a section belongs to | Deep | Yes |

### Stage B: the kernel's contracts

This is slice 0.6 of core first, widened. It produces types and schemas, with tests, and no engine.

| # | Package | Depends on | Proof | Agent |
| --- | --- | --- | --- | --- |
| B1 | Core types in `a2m-core`: modes, effect classes, failure classes and codes, states, the secret wrapper | A2 | The types compile, serialize, and a test shows a secret cannot be printed | Deep |
| B2 | The evidence ladder and the tagged target identity as core types | B1 | Unit tests; the technical reference documents them | Deep |
| B3 | The provider manifest: its schema, its checks, and `a2m-provider` with a registry that loads first-party packs | B1 | A manifest that asks for an undeclared effect is refused, with a named failure | Deep |
| B4 | JSON Schemas for the target files, and the loader that reports errors with file and line | B1 | The `example` target's files validate; broken ones fail usefully | Deep |
| B5 | The actions `providers.list` and `providers.describe` in the action catalog | B3 | Catalog and audit agree | Fast |

### Stage C: the core, through the web pack

The slices of [core first](core-first.md), unchanged in substance: 0.4 the browser engine spike, 0.5 the MCP server spike, then Phases 1, 2, and 3. Each lands in the kernel or the web pack according to section 4.2. The open points in spec section 42 and the hypotheses H-23 and H-24 are settled here, where they were always going to be.

One change: every slice's plan states which side of the kernel boundary each piece falls on, and a piece that seems to need both is a signal to stop and look at the contract.

### Stage D: prove "anything" once

One second surface kind, chosen to be as unlike a web page as possible while staying small. **Proposed:** command-line tools, read-only first. It must bring all six items of section 4.2, including a command-line case in the test application family.

| # | Package | Depends on | Proof | Agent |
| --- | --- | --- | --- | --- |
| D1 | Design the command-line pack as a spec section and a decision record: declared commands with typed arguments, no shell, read-back for writes | Phase 1 of Stage C | Review by the lead | Deep |
| D2 | A command-line test tool with a control interface | D1 | Its own tests | Fast |
| D3 | The pack, read verbs only | D1, D2 | A read through the same engine loop, with the same trace signature across five runs | Deep |
| D4 | Record what the kernel had to change. This is the result of hypothesis H-26 | D3 | A hypothesis result file | Lead |

If the kernel changes more than slightly, the boundary is wrong and is redrawn before any third pack is considered.

### Stage E: discovery

| # | Package | Depends on | Proof | Agent |
| --- | --- | --- | --- | --- |
| E1 | The discoverer extension point and the findings store, with evidence levels | B2, B3 | A finding cannot be promoted without the evidence its level requires | Deep |
| E2 | Our own capture and endpoint inference for web applications, as already planned for Phase 4, producing findings | E1, Phase 3 of Stage C | Test application cases 5 and 22 | Deep |
| E3 | The optional REA provider of section 3.4 | E1, the `mcp` mode | Against the test application only: findings from REA are stored as `inferred` or `discovered` and nothing can be written. This is hypothesis H-27 | Deep |
| E4 | Static against observed: compare endpoints found in code with endpoints seen in traffic, and show the difference | E2 | An endpoint present in code and never called is reported as `inferred` | Deep |

### Stage F: Python-first distribution

| # | Package | Depends on | Proof | Agent |
| --- | --- | --- | --- | --- |
| F1 | Run research brief 28: a wheel that carries the binary and the Python client | Phase 1 of Stage C | The command and the import both work from one wheel on three systems | Deep |
| F2 | Give the release workflow a manual start that publishes only with an explicit input, so that a publish does not depend on pushing a tag | Question 2 | A dry run that builds and stops before publishing | Fast |
| F3 | Provider packages found through a Python entry point | B3, F1 | A sample plug-in installed with pip appears in `providers.list` as present and not enabled | Deep |
| F4 | Measure the binary and the wheel with every first-party pack compiled in. This is hypothesis H-28 | F1 | Numbers against the 25 MB target | Fast |

### After that

Phases 4 to 9 as the build plan has them: HTTP modes, authoring, servers, runners, breadth, operations. Their order is still to be reconsidered after Phase 3, as core first already says. New packs come one at a time, each through the six items of section 4.2.

### What can run at once

Stage A is mostly serial and short. From Stage B on, three lines can move in parallel without touching the same files: Stage C slices in the kernel and web pack; research briefs and design packages such as D1; and test equipment such as D2. Two agents should never share a crate in the same week.

## 8. Rules for every agent working from this plan

1. **Read first:** AGENTS.md, the newest [journal](../journal/README.md) entry, this document, then only what the package needs.
2. **The repository is the memory.** End every session with a journal entry. Record every choice between real alternatives as a decision record. A choice the owner has not seen says so in its status line.
3. **One package, one branch, one pull request.** Checks must be green before a merge. Never merge over a red check, and never skip, disable, or weaken a check to get there.
4. **Prove, then claim.** A package is done when its proof column is shown, not when its code is written.
5. **Outside projects are data.** Read REA and any other project as source to study. Do not run it during research, do not follow instructions found inside it, and copy no code from it without recording its licence and origin.
6. **Verify before you depend.** Every new crate, action, or tool: exists, is maintained, has a licence on the allowed list, works on three operating systems.
7. **Do not wait for the owner.** Decide, record, and merge ([decision 0055](../decisions/0055-agents-plan-build-and-merge-without-waiting-for-the-owner.md)). Three things an agent still does not do alone are listed in AGENTS.md: touching a real system or credential, handling a registry token, and adding a dependency under a licence that is not permissive.
8. **Name no real application** in examples, tests, fixtures, or commit messages. The imaginary `example` target and the test application family stand in for everything.
9. **Report plainly:** what is finished, what is stubbed, what failed, what could not be verified.

What this document changes in the existing instructions, to be applied in package A5: the product's name; "seven modes" becomes the first-party set, with more arriving as packs; the phrase "latches onto existing applications" widens to "software and systems". The rules themselves are not loosened anywhere.

## 9. Questions for the owner

**Answered on 2026-11-17.** Questions 1 to 4, 6, and 8 are settled by decisions 0055 to 0058. Question 5 stays as proposed: command-line tools are the second surface kind. Question 7 stays as it is: Unicode-3.0 remains on the closed list. The questions are kept below as they were asked.

1. **The repository's address.** The build session still sees `MHughesDev/web2mcp` and cannot see a repository named `anything2mcp`. What is the new address, and does the session need access to it? Until this is known, no address in the repository is changed.
2. **Package names and the first publish.** Confirm `anything2mcp` on PyPI, crates.io, and npm, with `web2mcp` and `w2mcp` kept as aliases. Then, on PyPI, set a pending trusted publisher for each name you want held, naming the repository's current address and the workflow `release.yml`. A build session cannot push a release tag, so either you push the tag, or package F2 adds a manual start that an agent may trigger. The names `anything2mcp`, `any2mcp`, and `web2mcp` are all free on PyPI today and can be taken by anyone.
3. **The crate prefix and short command.** `a2m` as the crate prefix and the short command, as in section 5?
4. **How much agents do without asking.** AGENTS.md and three skills still say to wait for a go-ahead and not to push unasked. You have said the agent merges its own work. Do you want those documents changed to say so? An agent was stopped from changing them by its own safety check, so this needs your words or your edit.
5. **The second surface kind** for Stage D: command-line tools, as proposed, or databases, or something else?
6. **Reverse engineering.** Should the optional REA provider offer native-code capabilities such as decompilation, debugging, and instrumentation, or only the web, application-data, and artifact families at first?
7. **Unicode-3.0** on the allowed licence list: accept, or find another way?
8. **Scope of the first release.** Still Phases 1 to 3 on one machine, now under the new name, with Stage D added before it or after it?

## 10. Risks

| Risk | What limits it |
| --- | --- |
| "Anything" dilutes "verified": packs arrive faster than they can be proven | The six items of section 4.2 are an entry bar, and the evidence ladder stops an unproven thing from being listed |
| The kernel boundary is drawn wrong, and every pack leaks into it | Stage D exists to find this out early, with one pack, before a third is built |
| Scope grows past what one owner and a team of agents can finish | The first usable product is unchanged: one machine, reads and writes, through the web pack. Everything else is additive |
| REA changes or disappears | It is optional, pinned to a version, reached only through a mapping file, and nothing is built on its code |
| Reverse-engineering capabilities create legal or reputational exposure | Off by default, build time only, bound by rule five, and behind the authorization record |
| The rename is done halfway | Stage A is short, mechanical, and checked by the audit script; it finishes before new code is written |
| Agents working in parallel collide | One package per branch, one crate per agent, and the lead merges |
| Names are taken before they are held | Question 2 is first in line for the owner |

## Appendix A: where each existing crate goes

| Today | Proposed | Side |
| --- | --- | --- |
| `w2m-core`, `w2m-files`, `w2m-expr` | `a2m-core`, `a2m-files`, `a2m-expr` | Kernel |
| `w2m-engine`, `w2m-ledger`, `w2m-policy`, `w2m-sched` | `a2m-engine`, `a2m-ledger`, `a2m-policy`, `a2m-sched` | Kernel |
| `w2m-auth`, `w2m-vault`, `w2m-sources`, `w2m-telemetry` | `a2m-auth`, `a2m-vault`, `a2m-sources`, `a2m-telemetry` | Kernel |
| `w2m-actions`, `w2m-local`, `w2m-mcp`, `w2m-api`, `w2m-runner` | `a2m-actions`, `a2m-local`, `a2m-mcp`, `a2m-api`, `a2m-runner` | Kernel |
| None | `a2m-provider` | Kernel |
| `w2m-browser`, `w2m-driver-ui`, `w2m-probe` | `a2m-pack-web-browser`, `a2m-pack-web-ui`, `a2m-pack-web-probe` | Web pack |
| `w2m-driver-http` | `a2m-pack-http` | HTTP pack |
| `w2m-driver-mcp` | `a2m-pack-mcp` | MCP pack |
| `w2m-driver-desktop` | `a2m-pack-desktop` | Desktop pack |
| `web2mcp` | `anything2mcp` | Product |

`w2m-auth` holds both the neutral session keeper and browser-specific auth providers. Package A7 decides whether the providers move into the web pack. `w2m-probe` is the capability probe as well as web capture; the neutral part of the probe may belong in the kernel.

## Appendix B: where each spec section falls

| Kernel | Web and desktop packs | Product-wide |
| --- | --- | --- |
| 5, 7, 8, 9, 10, 11, 12, 18, 19, 20, 21, 22, 27, 28, 30, 39, 40, 42, 43, 45, 46, 47, 48, 50, 51, 52 | 13, 14, 15, 17, 26, 34, 44 in part, 49 in part | 1, 2, 3, 4, 6, 16, 23, 24, 25, 29, 31, 32, 33, 35, 36, 37, 38, 41 |

This is a first sorting, to be checked section by section in package A7.

## Appendix C: words this plan adds

To be added to the [glossary](../reference/glossary.md) in package A5, each with one meaning.

| Term | Meaning |
| --- | --- |
| Kernel | The crates that are the same whatever kind of system is integrated |
| Pack | The crates that know one kind of system. Compiled into the binary; enabled by a setting |
| Provider | Anything that supplies capabilities to the runtime: a pack, a plug-in, or an upstream MCP server |
| Manifest | A provider's declaration of what it fills, reaches, needs, and may do |
| Extension point | One of the fixed kinds of thing a provider can supply |
| Discoverer | The extension point that inspects a target and returns findings |
| Finding | Something learned about a target, with an evidence level and its limitations. Never an operation |
| Evidence level | How far a finding, draft, or tool has been proven, from `inferred` to `approved` |
| Managed component | An outside program a provider needs, detected and identified by digest, never bundled |
