# 106. What it latches onto

Anything2MCP reaches any existing system, from a device's firmware to a hosted service, without changing what makes it trustworthy. This section sets out the levels at which a system can be reached, every kind of system considered so far with what supporting it takes, what is refused, and what the catalog shows. The owner's decision is [record 0048](../../decisions/0048-the-product-becomes-anything2mcp.md); the scope is [record 0058](../../decisions/0058-a-toolkit-for-developers-with-no-catalog-and-no-preferred-agent.md). Which kinds of system have a built pack is in the table of packs in the [README](../../../README.md); none has been tried against a real system.

## The claim

Given an existing system, Anything2MCP produces tools an AI agent can call, where each tool is a deterministic operation that has been proven to do what it says.

"Existing" means the system is not changed to suit the toolkit. It is reached the way it can already be reached, through a surface the user is entitled to use.

## The stack, from the bottom up

A system can be reached at many levels. Each level has its own idea of what an operation is.

| Level | Examples | An operation is | Section |
| --- | --- | --- | --- |
| Devices and firmware | A controller on a fieldbus, a board on a serial console, a network device | Reading or setting a parameter, a declared console command, an update through the device's own mechanism | [310](../3-packs/310-the-device-pack.md) |
| Operating system | Services, packages, scheduled jobs, system settings, files | A declared command with typed arguments | [306](../3-packs/306-the-command-pack.md) |
| Native programs and libraries | A command-line tool, a shared library, a program with an automation interface | Starting it with arguments, or calling a function it exports | [201](../2-kernel/201-archetypes-and-modes.md), [306](../3-packs/306-the-command-pack.md) |
| Language runtimes and packages | A package or a script the system ships | A fixed script started by a fixed interpreter; a function with a typed signature is cataloged, as the library row below | [306](../3-packs/306-the-command-pack.md) |
| Data stores | Relational and other databases, files in known formats, queues | A query, a keyed write, a value at a named place, a message | [307](../3-packs/307-the-data-pack.md), [308](../3-packs/308-the-file-pack.md), [309](../3-packs/309-the-stream-pack.md) |
| Services and protocols | HTTP and its descendants, remote procedure calls, mail, directories, an MCP server | A typed request and its response | [303](../3-packs/303-the-http-pack.md), [304](../3-packs/304-the-mcp-pack.md) |
| Applications | Web, desktop, mobile, and terminal applications | What a person does on a screen, done by element and never by pixel | [301](../3-packs/301-the-web-pack.md), [305](../3-packs/305-the-desktop-pack.md) |
| Systems of systems | An order that touches an application, its database, and a device | A pipeline over several targets | [401](../4-building-tools/401-overlaying-tools-on-an-existing-system.md) |

The same target can be reached at several levels at once. An application has an interface, a backend, a database, perhaps an administration tool and a device behind it. [Section 401](../4-building-tools/401-overlaying-tools-on-an-existing-system.md) says how one target holds all of them and which level a tool should use.

## One method, one engine

Two things are the same at every level, and they are the product.

- **One method for building a tool:** attach, observe, model, bind, verify, publish, watch ([section 401](../4-building-tools/401-overlaying-tools-on-an-existing-system.md)). Its mechanical half runs as the scout ([section 402](../4-building-tools/402-the-scout-and-who-does-what.md)).
- **One engine for running it:** the engine loop of guard, choose, prepare, intend, act, confirm, read back, record, with intent before action and read-back after it ([section 207](../2-kernel/207-the-engine-loop.md)). The engine does not know what kind of system it is driving; its handling differs by archetype and never by mode ([section 201](../2-kernel/201-archetypes-and-modes.md)).

Everything that differs between kinds of system lives in a pack ([section 202](../2-kernel/202-kernel-and-packs.md)) behind one provider contract ([section 203](../2-kernel/203-the-provider-contract.md)). A kind becomes supported only by clearing the bar a new pack must clear, set in section 202.

## The catalog of kinds

**State** is one of:

| State | Meaning |
| --- | --- |
| Designed | A pack section designs it. It becomes real when the pack is built and clears the bar; a mode it proposes becomes real by its own decision record |
| Cataloged | Considered, not designed. An entry here is a plan to design a pack, not a design |
| Through another kind | Needs no pack of its own; reached through a pack already designed |
| Refused | See "Refused" below |

### Applications

| Kind | Archetype | Reached through | Read-back | Discovery | Test stand | State |
| --- | --- | --- | --- | --- | --- | --- |
| Web application | Interface | The browser's DevTools protocol, by role and name; the requests the page itself makes; tools a site publishes. Modes `ui`, `page_http`, `site_tools` | The field, or the save response | Snapshots; captured traffic | The test application | Designed: the web pack, [section 301](../3-packs/301-the-web-pack.md) and [section 302](../3-packs/302-the-web-pack-browser-engine.md). Built first |
| Desktop application | Interface | The operating system's accessibility tree. Mode `desktop` | The element's value | The accessibility tree | Test application case 28 | Designed: the desktop pack, [section 305](../3-packs/305-the-desktop-pack.md). Reads built on Linux only ([decision 0198](../../decisions/0198-the-desktop-pack-reads-linux-applications-through-a-look-only-back-end.md)); Windows and macOS are owed before the first release ([decision 0208](../../decisions/0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md)) |
| Mobile application | Interface | The platform's own automation interface on a device or emulator the person controls | The element's value | The interface tree | An emulator image with a test application | Cataloged |
| Terminal application | Interface | A pseudo-terminal: the screen as a grid of text, fields found by label and position within a named screen | The field on the screen | Screen captures | A small terminal program with a control interface | Cataloged. The command pack never gives a program a terminal |
| Application with an automation interface | Bridge | The interface the application offers for automation or add-ins | The matching read function | The interface's own type library or dictionary | A host with a documented scripting interface | Cataloged |

An application built on a browser runtime is a web surface.

### Services and protocols

| Kind | Archetype | Reached through | Read-back | Discovery | Test stand | State |
| --- | --- | --- | --- | --- | --- | --- |
| HTTP interface, described or not | Request | Modes `http` and `api` | The matching read request | Published descriptions; captured traffic | The test application's JSON interface | Designed: the HTTP pack, [section 303](../3-packs/303-the-http-pack.md) |
| Graph, procedure-call, and document-style web services | Request | `api`, from the service's published schema | The matching query | The schema; reflection where offered | Small additions to the HTTP stand | GraphQL and OData designed, as import formats of the HTTP pack; gRPC after the first release ([section 303](../3-packs/303-the-http-pack.md)); SOAP and JSON-RPC cataloged, and SOAP, whose XML answers the HTTP pack now reads, waits for XML request bodies |
| An existing MCP server | Request | Mode `mcp`; only tools named in a mapping file are reachable | The read tool named in the binding | The server's own tool list | A small MCP server under `targets/` | Designed: the MCP pack, [section 304](../3-packs/304-the-mcp-pack.md). Built over HTTP and, on Unix, for a local program; a local program on Windows is not built |
| Tools a site publishes | Request | Mode `site_tools`, from inside the page | The read tool named in the binding | The page's own list | The test application | Designed: the web pack |
| Directory | Request | A directory protocol client | A lookup of the entry | The schema the directory publishes | Not designed | Cataloged |
| Mail | Request for reading; stream for sending | A mailbox protocol; a submission protocol | None for sending: `irreversible` | Folder lists | Not designed | Cataloged |
| Hosted services and cloud platforms | Request or command | Their HTTP interface, or their own command-line tool | As for those kinds | As for those kinds | As for those kinds | Through another kind |

### Data

| Kind | Archetype | Reached through | Read-back | Discovery | Test stand | State |
| --- | --- | --- | --- | --- | --- | --- |
| Relational database | Request | Mode `sql`: a driver for the database, with declared statements and typed parameters; never text assembled from inputs | A query by key | The database's own catalog of tables, columns, and keys, read in a read-only transaction | An embedded database with seed tables and a control interface | Designed: the data pack, [section 307](../3-packs/307-the-data-pack.md). Reads first |
| Document and key-value stores | Request | The store's client protocol | A read by key | The store's listing of collections; sampled documents, marked `inferred` | A local instance | Cataloged. A mode of its own follows relational writes ([section 307](../3-packs/307-the-data-pack.md)) |
| Files in known formats | Artifact | Mode `file`: reading the file, and saving by writing a new file that replaces the old one in one step | Open the saved file again | The file's own structure: sheets, named ranges, keys | Files in a temporary folder | Designed: the file pack, [section 308](../3-packs/308-the-file-pack.md) |
| Object storage | Request | The storage interface | Read the object's digest | Listings | A local emulator | Through the HTTP kind, which reads XML listings since [decision 0217](../../decisions/0217-the-http-pack-reads-answers-that-are-not-json.md), once it signs each request, which is not designed ([section 303](../3-packs/303-the-http-pack.md)) |
| Queues, topics, change feeds, and logs | Stream | Mode `stream`: the broker's client protocol | Only where the system offers a read: a retained value for a key, a stored message | Channel lists; registered schemas; never consuming | A small broker with a control interface | Designed: the stream pack, [section 309](../3-packs/309-the-stream-pack.md) |
| Search indexes | Request | The index's interface | A query by identifier | The index's mappings | A local instance | Through the HTTP kind |

**Writing beneath an application.** A database or a file that an application owns is a lower level of that application. Writing to it directly skips the application's rules. Its surface therefore declares where it stands: alone, as the system of record, or beneath the application on a named surface. Beneath an application, reads are free and a write compiles only where the application's owner has said the route is supported; undeclared, no write compiles ([section 401](../4-building-tools/401-overlaying-tools-on-an-existing-system.md), [section 307](../3-packs/307-the-data-pack.md), [section 308](../3-packs/308-the-file-pack.md)).

### Programs and the operating system

| Kind | Archetype | Reached through | Read-back | Discovery | Test stand | State |
| --- | --- | --- | --- | --- | --- | --- |
| Command-line tool | Command | Mode `cli`: starting the program by absolute path, checked by digest and version, with typed arguments; never a shell | A declared read command | The tool's own help and version output, in the forms the target declares | `example-tool`, a small tool with a control interface | Designed: the command pack, [section 306](../3-packs/306-the-command-pack.md). The second pack to be built |
| The same on another machine | Command | A runner on that machine, the session's home runner, which starts the program locally by the same rules. Never a remote login, which hands a line to the remote shell | As above | As above | As above | Designed, for later: [section 306](../3-packs/306-the-command-pack.md) |
| Library or package | Bridge | Calling an exported function with typed arguments in a helper process | A matching read function | Headers, type stubs, signatures | A small library | Cataloged |
| Services, packages, scheduled jobs, system settings | Command or bridge | The operating system's own tools and interfaces | The matching query | Listings | A container or virtual machine | Through another kind |
| Containers and orchestrators | Request or command | Their interface or their tool | As for those kinds | As for those kinds | A local cluster | Through another kind |

### Devices

[Section 310](../3-packs/310-the-device-pack.md) gives the rules and the pack. A device is reached through its own interfaces, and a firmware update only through the device's own documented mechanism ([record 0054](../../decisions/0054-devices-through-their-own-interfaces-and-firmware-through-the-devices-own-update.md)).

| Kind | Archetype | Reached through | Read-back | Test stand | State |
| --- | --- | --- | --- | --- | --- |
| Device with a management protocol over a network | Request | The protocol the device documents, through the request packs, under the device rules of section 310 | Read the same object | As for the request pack | Through another kind |
| Console reached by starting a program | Command | The command pack, which starts the program locally | A declared query command | As for the command pack | Through another kind |
| Console on a serial line or a raw socket, with no program between | Device | Mode `device`: a framed link whose frames are lines of text, one declared command out and one answer in, parsed by a strict pattern | A declared query command | A simulated device | Designed: the device pack |
| Device on a fieldbus | Device | Mode `device`: registers or objects by address, from the device's published map | Read the register again | A simulated device with registers and a simulated process | Designed: the device pack |
| Firmware image | Not an operation | Inspection only, as discovery, through an optional provider that is off by default | Not applicable | Not applicable | Designed: the device pack |
| Firmware update | Device or request | The device's own documented update mechanism only | The version and digest the device reports afterwards | The simulated device's two-slot update mechanism | Designed: the device pack. Always `irreversible` |

## Refused

| Kind | Why |
| --- | --- |
| Clicking by pixel or image | Nothing improvises, and nothing can be verified against a picture |
| Changing a running program's memory or code; attaching to a program already running to read its memory | It is not an interface the system offers; nothing can be verified against it |
| Writing through a debug probe or an exploit | The worst outcome is permanent ([section 310](../3-packs/310-the-device-pack.md)) |
| Anything that defeats a protection: sign-in, bot defences, copy or licence checks, secure start-up, read-out protection | Rule five. A block is a named stop |
| A general shell, a command interpreter, or a statement built from an agent's text | Being able to run a command is not permission to run it ([section 213](../2-kernel/213-effects-hazards-and-consent.md)) |

## What "anything" does not mean

| It does not mean | Because |
| --- | --- |
| Anything unverified | A kind of system is supported only when a write to it can be read back, or is honestly classed `irreversible`. A pack that cannot read back its writes can still be accepted for reads |
| A model deciding at run time | Rule one. Discovery and drafting may use any help; a run uses none |
| A shell | Commands are declared, with typed arguments |
| Getting in where it is not let in | Rule five. Nothing learned by inspection is used to defeat a protection |
| Everything at once | One pack is proven before the next begins. The first usable product is reads and writes on one machine through the web pack |

## What the catalog shows

- Most kinds need no pack of their own. Hosted services, cloud platforms, containers, object storage, search indexes, operating-system services, and devices with a management protocol are reached through the request and command packs.
- Every mechanism by which a program on a personal computer can read or change another, with its place in this design today, is in the [coverage map](../../reviews/coverage-map.md).
- Five packs beyond the first four cover much of the rest: command, data, file, stream, and device. The map shows what they do not: above all the operating systems' scripting interfaces, answers that are not JSON, and files that are not tables. All five are designed ([record 0064](../../decisions/0064-five-further-packs-are-designed-before-they-are-built.md)); bridge, mobile, and terminal follow where there is demand.
- The command pack was chosen as the second pack to build. It is as unlike a web page as possible, it is small, and many other kinds can be reached through it. Its job is to show that the kernel stays neutral once a second kind of system exists; the kernel changes it forced are counted in [decision 0099](../../decisions/0099-the-command-pack-as-building-it-settled-it.md), and the result file of hypothesis H-26 (package D4) is not yet written.

## Open points

- Whether every system of interest fits one of the seven archetypes. Hypothesis H-29.
- Whether the kernel can stay neutral once a second pack exists. Hypothesis H-26, tested by Stage D of the [long-term plan](../../plan/roadmap.md).
- The order of building after the command pack. The data pack, reads only, is the likely next.
- Whether the terminal kind is its own mode or an interface mode of the command pack.
- Which mobile automation interface, and whether real devices are in scope or emulators only.
- Whether `library` is a mode of its own or the bridge mode with the host being a language runtime.
