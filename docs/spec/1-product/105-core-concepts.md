# 105. Core concepts

These are the nouns of the system. The developer, or an agent working for them, writes the target files; the kernel and the packs supply the rest. Each term has one meaning, given in the [glossary](../../reference/glossary.md); code, files, and messages use no synonyms. A concept is built where the section that designs it says ([section 704](../7-project/704-readiness.md)).

## What the developer writes

| Concept | What it is | Example |
| --- | --- | --- |
| Target | One system being wrapped: an application, a service, a program, a data store, a file, a broker, a device. A target is a folder of target files | `example` |
| Surface | One way into a target: a web origin, an API base address, an MCP server, a desktop window, a program, a database, a file, a broker, a device link. A target can have several | `app.example.com`, `api.example.com`, the program `example-tool` |
| Surface kind | What kind of surface it is. A registered name: the pack that serves the kind registers it ([decision 0062](../../decisions/0062-words-that-arrive-with-packs-are-registered-names-and-packs-share-data-never-code.md)) | `web`, `http`, `mcp`, `desktop`; `program`, `broker`, `device_link` from later packs |
| Identity | Who the target sees: the person, or an account of the toolkit's own | A named person, signed in through the organization's single sign-on |
| A pack's file kinds | The files in which a pack keeps what it knows about a surface of its kind. Each kind has one owning pack; another pack may read it as data. The kernel's own kinds are `target.yaml`, `auth.yaml`, `types.yaml`, `operations/`, `pipelines/`, `sources/`, `tests/`, and `fixtures/` | See the table below |
| Field type | How a value is entered, read back, and compared: kind, unit, scale, rounding, locale, control type. Units live only here | `length_ft_in` |
| Operation | One business action with typed inputs and outputs, an effect class, and its hazards, defined once whatever mode carries it out | `example_line_item_upsert` |
| Binding | How one mode carries out one operation, with its own steps and verification. An operation can have several, tried in the order the target declares | A `ui` binding and an `http` binding of `example_line_item_upsert` |
| Pipeline | Operations in order over a source, with checkpoints and success checks | `example_order_sheet_enter` |
| Source | A reader that turns an input file or another system's output into validated records | The spreadsheet reader |

### The file kinds packs bring

These are examples. Each pack's section defines its own.

| File kind | Pack | Describes | Example |
| --- | --- | --- | --- |
| Screen model, `screens/` | Web; desktop | A named page, panel, or window: how to reach it, its fields and actions, how to know it is ready. Locators live only here | `order.line_item_form` |
| Endpoint, `endpoints/` | HTTP; the web pack's `page_http` mode reads it | A named request: method, path template, request and response schemas | `PUT /api/orders/{id}/items/{code}` |
| Mapping file, `mappings/` | MCP | Which tools of an upstream MCP server may be reached, and as what, each pinned by the digest of its definition. No other tool of that server is reachable | One file per `mcp` surface of `example` |
| Command, `commands/` | Command | A program's commands, each with typed arguments and a declared output | `item_show` in `commands/inventory.yaml`, for `example-tool` |
| Statement, `statements/` | Data | The record kinds on a database and every statement a binding may run | `statements/orders.yaml` |
| Document, `documents/` | File | One file: its format, its fingerprint, and its named places | `documents/prices.yaml` |
| Channel, `channels/` | Stream | Where messages are on a broker and what they look like | `orders.status` |
| Register map, `registers/` | Device | Every address a device pack may touch, its encoding, range, and hazard | `controller.main` |

## What the kernel and the packs supply

| Concept | What it is | Example |
| --- | --- | --- |
| Access kind | One of the seven ways a system is reached: interface, request, command, bridge, file, stream, device. It fixes how a step is acted, confirmed, and read back. A closed type in the kernel ([section 201](../2-kernel/201-access-kinds-and-modes.md)) | Interface |
| Mode | One concrete way of reaching a system, brought by a pack and belonging to one access kind. A registered name: the kernel holds no list of modes | `ui`, `http`, `mcp` |
| Pack | The crates that know one kind of system. Compiled into the binary, enabled by a setting ([section 202](../2-kernel/202-kernel-and-packs.md)) | The web pack |
| Provider | Anything that supplies capabilities to the runtime through the provider contract: a pack, a plug-in, or an upstream MCP server ([section 203](../2-kernel/203-the-provider-contract.md)) | A plug-in for a rare kind of system |
| Driver | Pack code that carries out the steps of one or more modes. Only the engine calls it | The browser interface driver; the HTTP driver |
| Auth provider | Code for one sign-in method: how a session of one kind is made, checked, renewed, and ended. Only the session keeper calls it | A single sign-on provider |
| Session | The signed-in state of one identity on one target in one environment, shared by many runs and held on its home runner ([section 215](../2-kernel/215-sessions-and-the-session-keeper.md)) | One person's session on `example` |
| Compiled target | The compiled, hashed form of a target version. The engine runs only compiled targets ([section 206](../2-kernel/206-the-compiler-and-the-compiled-target.md)) | The compiled target of `example@12` |
| Version | An immutable, tested, published snapshot of a target's files | `example@12` |
| Run | One call of an operation or pipeline, with its ledger, trace, and result | `r_8f21` |
| Finding | Something learned about a target, with its subject, an evidence level, its source, and its limitations. A finding never runs ([section 218](../2-kernel/218-discovery-and-findings.md)) | "The surface `api.example.com` publishes a description listing a read of an order" |
| Evidence level | How far a finding, draft, or tool has been proven: `inferred`, `discovered`, `reconstructed`, `schema_verified`, `executed`, `behavior_verified`, `approved`. Earned, never declared; it informs a person at review and gates no listing or call ([section 219](../2-kernel/219-the-evidence-ladder.md)) | A drafted read that passed five identical runs in the sandbox is `executed` |

Three further words are used everywhere: an **effect class** is `read`, `write`, or `irreversible`; a **hazard** is what an operation could damage beyond the data it names ([section 213](../2-kernel/213-effects-hazards-and-consent.md)); and an **record matching** says how a written record is found again ([section 209](../2-kernel/209-record-identity-and-dependent-records.md)).

## Verbs

A binding names one verb. Each verb carries its own wait, commit, and verification rules in every driver, so an operation never spells them out. A pack lowers each verb it offers into the steps of its mode.

The built-in verbs: `open`, `read`, `list`, `search`, `create`, `update`, `upsert`, `delete`, `upload`, `download`, `submit`, `call`.

**Each pack states which verbs it offers.** A binding with a verb its mode's pack does not lower does not compile. A pack may bring a verb of its own, such as waiting for a message on a stream; how that sits beside the built-in list is the open point below.

## Open points

- Whether the verb is a closed list or a checked name that packs register, with the engine's treatment declared by each lowering rather than read from the verb's spelling. The crate design for [`a2m-core`](../../architecture/crates/a2m-core.md) gives both sides; package 1.1 of the [roadmap](../../plan/roadmap.md) decides.
