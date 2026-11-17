# 62. Catalog of surface kinds

Every kind of system considered so far, with what supporting it would take. This is a catalog, as section 13 is for interface flavors: an entry here is a plan to design a pack, not a design. A kind becomes real only by clearing the bar in [section 55](55-kernel-and-packs.md).

**State** is one of: *designed* (sections 1 to 52 cover it), *next* (proposed as an early pack), *cataloged* (considered, not designed), *through another kind* (needs no pack of its own), *refused*.

## Applications

| Kind | Archetype | Reached through | Read-back | Discovery | Test stand | State |
| --- | --- | --- | --- | --- | --- | --- |
| Web application | Interface | The browser's DevTools protocol, by role and name | The field, or the save response | Snapshots; captured traffic | The test application | Designed |
| Desktop application | Interface | The operating system's accessibility tree | The element's value | The accessibility tree | Test application case 28 | Designed as a catalog |
| Mobile application | Interface | The platform's own automation interface on a device or emulator the person controls | The element's value | The interface tree | An emulator image with a test application | Cataloged |
| Terminal application | Interface | A pseudo-terminal: the screen as a grid of text, fields found by label and position within a named screen | The field on the screen | Screen captures | A small terminal program with a control interface | Cataloged |
| Application with an automation interface | Bridge | The interface the application offers for automation or add-ins | The matching read function | The interface's own type library or dictionary | A host with a documented scripting interface | Cataloged |

## Services and protocols

| Kind | Archetype | Reached through | Read-back | Discovery | State |
| --- | --- | --- | --- | --- | --- |
| HTTP interface, described or not | Request | `http`, `page_http`, `api` | The matching read request | Descriptions; captured traffic | Designed |
| Graph, procedure-call, and document-style web services | Request | `api`, from the service's published schema | The matching query | The schema; reflection where offered | Designed as import formats |
| An MCP server; tools a site publishes | Request | `mcp`, `site_tools` | The read tool named in the binding | The server's own tool list | Designed |
| Directory | Request | A directory protocol client | A lookup of the entry | The schema the directory publishes | Cataloged |
| Mail | Request for reading; Stream for sending | A mailbox protocol; a submission protocol | None for sending: `irreversible` | Folder lists | Cataloged |
| Hosted services and cloud platforms | Request or Command | Their HTTP interface, or their own command-line tool | As for those kinds | As for those kinds | Through another kind |

## Data

| Kind | Archetype | Reached through | Read-back | Discovery | Test stand | State |
| --- | --- | --- | --- | --- | --- | --- |
| Relational database | Request | A driver for the database, with declared statements and typed parameters; never text assembled from inputs | A query by key | The database's own catalog of tables, columns, keys | A local database with a control query | Cataloged; reads first |
| Document and key-value stores | Request | The store's client protocol | A read by key | The store's listing of collections; sampled documents, marked `inferred` | A local instance | Cataloged |
| Files in known formats | Artifact | Reading and writing the file | Open the saved file again | The file's own structure: sheets, named ranges, keys | Files in a temporary folder | Cataloged; the source readers already read some |
| Object storage | Request | The storage interface | Read the object's digest | Listings | A local emulator | Through the HTTP kind |
| Queues and streams | Stream | The broker's client protocol | Only for keyed, retained messages | Topic lists; registered schemas | A local broker | Cataloged |
| Search indexes | Request | The index's interface | A query by identifier | The index's mappings | A local instance | Through the HTTP kind |

**Writing beneath an application.** A database or a file that an application owns is a lower level of that application. Writing to it directly skips the application's rules. [Section 60](60-overlaying-tools-on-an-existing-system.md) allows reads there freely and writes only where the application's owner has said the route is supported.

## Programs and the operating system

| Kind | Archetype | Reached through | Read-back | Discovery | Test stand | State |
| --- | --- | --- | --- | --- | --- | --- |
| Command-line tool | Command | Starting the program with typed arguments; no shell | A declared read command | The tool's own help, manual, and completion data | A small tool with a control interface | Next |
| The same on another machine | Command | A remote session the session keeper holds | As above | As above | A local container | Cataloged |
| Library or package | Bridge | Calling an exported function with typed arguments in a helper process | A matching read function | Headers, type stubs, signatures | A small library | Cataloged |
| Services, packages, scheduled jobs, system settings | Command or Bridge | The operating system's own tools and interfaces | The matching query | Listings | A container or virtual machine | Through another kind |
| Containers and orchestrators | Request or Command | Their interface or their tool | As for those kinds | As for those kinds | A local cluster | Through another kind |

## Devices

Section 63 gives the rules. In brief:

| Kind | Archetype | Reached through | Read-back | State |
| --- | --- | --- | --- | --- |
| Device with a management protocol | Request | The protocol the device documents | Read the same object | Cataloged |
| Device with a console | Command over a link | Declared console commands on a serial or network console | A declared query command | Cataloged |
| Device on a fieldbus | Device | Registers or objects by address, from the device's published map | Read the register again | Cataloged |
| Firmware image | Not an operation | Inspection only, as discovery | Not applicable | Cataloged, through an optional provider |
| Firmware update | Device or Request | The device's own documented update mechanism only | The version and digest the device reports afterwards | Cataloged; always `irreversible` |

## Refused

| Kind | Why |
| --- | --- |
| Clicking by pixel or image | Section 3: nothing improvises. A stated non-goal |
| Changing a running program's memory or code | It is not an interface the system offers; nothing can be verified against it |
| Writing through a debug probe or an exploit | Section 63 |
| Anything that defeats a protection: sign-in, copy or licence checks, secure start-up, read-out protection | Rule five |
| A general shell, or a statement built from an agent's text | Section 61 |

## What the catalog shows

- Most kinds need no pack of their own. Hosted services, cloud platforms, containers, object storage, and search are reached through request or command packs already planned.
- Five packs beyond the first four would cover the rest: command, database, file, stream, device. Bridge, mobile, and terminal follow where there is demand.
- The first to build is the command pack. It is as unlike a web page as possible, it is small, and half the other kinds can be reached through it.

## Open points

- The order after the command pack. Databases, read-only, are the likely second.
- Whether the terminal kind is its own mode or an interface flavor of the command pack.
- Which mobile automation interface, and whether real devices are in scope or emulators only.
