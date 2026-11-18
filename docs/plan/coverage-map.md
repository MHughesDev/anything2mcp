# Coverage map

Every way a program on a person's own computer can read or change another piece of software, or the system itself, and where each falls in this design today. It answers the owner's direction of [decision 0208](../decisions/0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md): "literally anything their machine can read", on Windows, macOS, and Linux, with the MCP served on that machine only.

It is a map, not a design. A row says where a mechanism would go and what closing its gap takes; the design of each is written when it is built, by the [working method](working-method.md). The findings drawn from it, and the records it raised, are in the [coverage review of 2026-11-18](coverage-review-2026-11-18.md). What other projects already reach, and how popular each is, is in [research note 27](../research/27-the-public-landscape-of-agent-tools.md); the review says where this map and that note differ (on the scripting interfaces' archetype and on the size of gRPC). Update a row in the pull request that changes its place.

## How to read it

| Column | Meaning |
| --- | --- |
| Read · change | How the mechanism is read, and how it is changed |
| Describes itself | Whether the mechanism publishes a description a scout could import, and by what. **Reads stated** means the description itself says which calls only read, so reads can be drafted and tried by rule ([section 402](../spec/4-building-tools/402-the-scout-and-who-does-what.md)) |
| OS | W Windows, M macOS, L Linux |
| Place today | **B** built (code and tests, on the systems named); **D** designed, not built; **T** through another kind; **C** cataloged in [section 106](../spec/1-product/106-what-it-latches-onto.md), not designed; **N** no place; **R** refused. Then the pack, mode, and archetype |
| To close it | The kind of change and its size, S, M, or L: a discoverer and drafting rule over an existing mode; a new mode in a pack; a new pack; a kernel change. "Leave out" means not in the first release |
| Rules | Which of the [ten rules](../../AGENTS.md#rules-that-never-bend) bear on it |

## Network interfaces a program speaks

| Mechanism | Read · change | Describes itself | OS | Place today | Gap | To close it | Rules |
| --- | --- | --- | --- | --- | --- | --- | --- |
| HTTP with an OpenAPI description | A safe method · a declared write endpoint | OpenAPI; **reads stated** by method | WML | B: HTTP pack, `http` and `api`, request. Import drafts a read per GET | Writes never drafted, by design | None | 3, 7 |
| HTTP with another description (RAML, API Blueprint, Postman or HAR collections, OpenRPC) | As above | Yes; reads stated for most | WML | N | No importer | Discoverer and drafting rule over `api`, S each, after the shared shape of H-41 | 7 |
| HTTP with no description | Captured requests · declared endpoints | No | WML | B in part: endpoints drafted from the web pack's capture, `inferred` | A program that is not the engine's browser cannot be captured without a proxy in the middle | Leave out | 4, 5 |
| HTTP whose answers are not JSON (XML, text, binary) | — | — | WML | N: "a body that is not JSON fits no schema" ([section 303](../spec/3-packs/303-the-http-pack.md)) | Blocks object stores, feeds, SOAP, DAV, printers, device pages | Answer formats in the HTTP pack, M | 7 |
| HTTP over a local socket or named pipe | As HTTP | Sometimes (container engines publish OpenAPI) | WML | N | No surface address for a socket or pipe | Surface field in the HTTP pack, S to M | 2 (a container engine's socket gives administrator rights: a hazard) |
| GraphQL | Query · mutation | Introspection; **reads stated** | WML | D: HTTP pack import | Not built | Discoverer and rule over `api`, S to M | 7 |
| gRPC | Unary call · call | Reflection, proto files; reads stated only where `idempotency_level` says so | WML | D, after the first release ([section 303](../spec/3-packs/303-the-http-pack.md)) | HTTP/2, protobuf, a stand | New request mode in the HTTP pack, M to L | 7 |
| SOAP | Operation · operation | WSDL; reads rarely stated | WML | N | XML answers; the envelope | After XML answers, an import, M | 7 |
| JSON-RPC | Call · call | Only with OpenRPC (`rpc.discover`) | WML | T over HTTP, as a declared POST whose effect the developer states; N over standard streams or a socket | No transport but HTTP | A request mode on the MCP pack's transports, M | 7 |
| WebSocket | Message · message | Rarely (AsyncAPI) | WML | N: observed only for confirmation in the web pack, refused in discovery | No surface kind | A request mode with a stream side, M | 3, 7 |
| Server-sent events | Wait for an event · — | Rarely | WML | N | No surface kind | A stream mode, S to M | 3 |
| OData | Entity set · entity write | `$metadata`; **reads stated**, keys stated | WML | D: HTTP pack import | Not built | Discoverer and rule over `api`, S | 7 |
| An existing MCP server | Tool · tool | Tool list; **reads stated** (`readOnlyHint`) | WML | B: MCP pack, `mcp`, request, over HTTP and a local program on Unix | A local program on Windows; resources and prompts | Finish the pack, S to M | 4, 7 |
| An identity provider's flows | — (sessions, not tools) | Discovery documents | WML | B in part: `a2m-auth`, the web pack's sign-in providers | Not tools by intent | None | 4, 5 |
| A directory (LDAP) | Lookup · entry write | Schema; reads stated by operation | WML | C | No mode | Request mode, M | 4 |
| A program's own protocol on a raw port | Frame · frame | No | WML | N, except text consoles in the device pack | — | Plug-in | 7 |

## A web application in a browser

| Mechanism | Read · change | Describes itself | OS | Place today | Gap | To close it | Rules |
| --- | --- | --- | --- | --- | --- | --- | --- |
| The page's screens | Element by role and name · set and invoke | Partly: the snapshot | WML | B: web pack, `ui`, interface; reads built, writes in progress | Widget drivers; platform profiles | Roadmap 1.6, 3.x, 8.4 | 1, 5, 7 |
| The page's own requests | Request from inside the page · the same | No: observed | WML | B: web pack, `page_http`, request, read half | Write half | Roadmap 4.2 | 5 |
| Tools a site publishes | Tool · tool | The page's list; **reads stated** | WML | D: web pack, `site_tools`, request (roadmap 8.2a) | Not built | S | 7 |
| The person's own signed-in browser | — | — | WML | R ([decision 0031](../decisions/0031-the-browser-always-runs-with-a-profile-the-engine-owns.md)) | Every site the person is already in needs a sign-in in the engine's profile | Owner's question, [decision 0211](../decisions/0211-whether-the-engine-may-ever-act-in-a-session-the-person-already-has.md) | 5 |
| Browser extensions' own interfaces | — | No | WML | N: the engine's profile holds none of the person's extensions | — | Leave out | 5 |
| An application built on a browser runtime | As a web page | As a web page | WML | D: web pack, started by the supervisor with its inspector open | Its own data directory is another program's store | Started with a data directory the engine owns, else refused; decision 0211 | 5 |
| A browser's own stores (history, bookmarks, cookies) | — | — | WML | R | — | None | 4, 5 |

## A desktop application

| Mechanism | Read · change | Describes itself | OS | Place today | Gap | To close it | Rules |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Accessibility tree, AT-SPI | Element by role and name · value pattern, invoke | The tree; no reads stated | L | B for reads: desktop pack, `desktop`, interface ([decision 0198](../decisions/0198-the-desktop-pack-reads-linux-applications-through-a-look-only-back-end.md)) | Writes | Roadmap 8.3, M | 5, 7 |
| Accessibility tree, UI Automation | As above | As above | W | D | Back end | Roadmap 8.3, L; owed before the first release (decision 0208) | 5, 7 |
| Accessibility tree, macOS | As above | As above | M | D | Back end; the person grants the permission | Roadmap 8.3, L; owed (decision 0208) | 5, 7 |
| Synthetic keys and clicks | — · a key to the focused window | No | WML | R until a key can be bound to its window (decision 0198) | Applications that save on Enter or leaving a field | Research, then the desktop pack, M | 5, 7 |
| Scripting dictionary and Apple events, JXA | Property get · set, command | Scripting dictionary; **reads stated** for properties (`access`) | M | C: section 106 puts it under bridge | No mode | A fixed script run by a fixed interpreter, S ([decision 0214](../decisions/0214-ways-through-the-refusals-that-keep-a-person-from-their-own-machine.md)); a mode, L | 5 (the person grants Automation), 6 |
| COM automation and type libraries | Property get · property put, method | Type library; **reads stated** (`propget`) | W | C, as above | No mode | Fixed script, S (decision 0214); a mode, L | 6 |
| D-Bus services | Property get · property set, method | Introspection; **reads stated** (`access="read"`) | L | N: D-Bus is used only under AT-SPI and polkit | No mode | Request mode, M; the D-Bus crate is in the workspace | 7 |
| An embedded scripting host (macro language, an application's own script console, add-ins) | Function · function | Sometimes, by documentation | WML | C: bridge, modes for planning | Our code would live inside the host, changing it | Leave out | 1, 7 |
| URL schemes and deep links | — · open a link | Partly: the scheme is registered, its parameters are not | WML | N: the openers start what their argument names, which the command pack refuses | No result comes back | Leave out | 7 |
| The system's action runners (Shortcuts and the like) | — · run a named shortcut | Partly: a list of names, untyped input | M (W, L less) | T: command pack, unproven | A shortcut's content is not pinned by digest | `irreversible` through the command pack, S | 3, 7 |
| The application's files and settings | A named place · replace the file | The format | WML | B for delimited text, JSON, workbooks (reads); D for YAML, TOML, XML (read only) | Property lists and INI have no place | Formats in the file pack, S each | 3 (writes beneath an application) |
| Its local database | A declared statement · keyed write | The store's catalog; reads stated by the read-only transaction | WML | B: data pack, `sql`, request | A write beneath the application needs its owner's word | None | 3 |
| Its local network port | As its protocol | As its protocol | WML | T: the request packs, on a loopback address | `localhost` refused as a name for plain links | Decision 0214, S | 4 |
| Window handles and messages | — · a message to a window | No | W | N | — | Leave out | 7 |
| Pixels on the screen | — | No | WML | R ([section 106](../spec/1-product/106-what-it-latches-onto.md)) | Applications that offer nothing else, remote desktops | Owner's question, [decision 0212](../decisions/0212-whether-an-application-that-offers-only-pixels-is-ever-reached.md) | 1, 7 |

## The command line

| Mechanism | Read · change | Describes itself | OS | Place today | Gap | To close it | Rules |
| --- | --- | --- | --- | --- | --- | --- | --- |
| A program's arguments and output | Declared read command · declared write with read-back | Help and version; no reads stated | WML | B: command pack, `cli`, command, on Linux; Windows and macOS not run | The path check passes everything on Windows; on macOS, application folders are group-writable (believed) | Decision 0214, research brief 35, M | 7 |
| Completions and manual pages | — | Partly: commands and flags | ML (W less) | D: discovery may read completion data; not built | — | Discoverer, S | 7 |
| Shell built-ins and PowerShell cmdlets | — | Cmdlets self-describe; reads by verb convention (`Get-`) only | W mostly | R: no shell | Most of Windows administration | A fixed script run by a fixed interpreter, S (decision 0214) | 6, 7 |
| A long-running program with a prompt | — | No | WML | R in the command pack (no terminal); the terminal kind is C | — | An interface mode over a pseudo-terminal, L; leave out | 7 |
| A terminal application's screen | Field on a text screen · keys | No | WML | C | — | As above; leave out | 7 |
| A program on another machine by a remote shell | — | — | WML | R; through a runner on that machine after the first release ([section 306](../spec/3-packs/306-the-command-pack.md)) | The machine reaches it; the design does not | Leave out; remote subsystems that take no shell are research | 5, 7 |
| Containers and their runtimes | Inspect · create, stop | The engine's interface publishes OpenAPI | WML | T: command pack for the tool, HTTP pack for the engine's interface once it can reach a socket | `exec` starts the program its argument names, refused | Socket surface (above) | 2 |
| Package managers | List · install, remove | Partly | WML | T: command pack | A system-wide install needs an administrator | Elevation, decision 0214 | 2, 5 |
| Version control | Log, status · commit, push | Partly | WML | T: command pack | The tool starts programs its configuration names (hooks) | Review in the target; S | 7 |
| Elevation (sudo, pkexec, an administrator prompt) | — | — | WML | R: a program that starts another as another user | Every change that needs an administrator | A fixed program elevated through the system's own prompt, M (decision 0214) | 5 |

## The operating system itself

| Mechanism | Read · change | Describes itself | OS | Place today | Gap | To close it | Rules |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Folders as collections of entries | List · create, rename, move, delete an entry | Names and metadata | WML | N: the file pack reads a file's content, never a folder's entries | — | A mode in the file pack, entries of a declared folder as records, M | 3, 7 |
| Processes | List · start, signal, end | No | WML | T: command pack; attaching is refused | — | None | 7 |
| Services | Status · start, stop, enable | systemd over D-Bus does; others no | WML | T: command pack | An action is a write of the state it leaves (H-42) | Drafting guidance, S | 3 |
| Scheduled tasks | List · add, remove | No | WML | T: command pack; launchd's files need property lists | Property lists | File format, S | 3 |
| The registry, the defaults database, dconf | Query · set | Typed values | WML | T: command pack (`reg`, `defaults`, `gsettings`) | — | None | 3 |
| Installed software | List · — | Partly | WML | T: command pack | — | None | — |
| Network settings | Query · set | NetworkManager over D-Bus | WML | T: command pack | Never the link the engine stands on ([section 310](../spec/3-packs/310-the-device-pack.md), rule 8 there) | None | 2 |
| Notifications | Others': protected · send one | Linux has a D-Bus specification | WML | T for sending, `irreversible`; reading others' is R | A person approves every one | Decision 0215, H-42 | 3, 4, 5 |
| Windows on the screen | List, title · move, focus | Partly, through accessibility | WML | B for reading a window in the desktop pack; no window operation | — | Verbs in the desktop pack, S | 5 |
| The clipboard | Read · write | No | WML | N; the desktop pack never touches it | It often holds a password | Leave out reads; a write as `irreversible`, S | 4 |
| The key store | The engine's own items | — | WML | B in part: the vault, the engine's own items only | Another program's items are refused | None | 4, 5 |
| Printing | Queue status · print | IPP describes the printer | WML | T: command pack, `irreversible` | — | None | 3 |
| System management (WMI and CIM; `/proc`, `/sys`; `sysctl`) | Query · set | WMI and CIM: class schemas, reads stated by property | WML | N for WMI; `/proc` text is no supported format | — | WMI with the COM route; text formats in the file pack, M | 7 |
| Event logs | Query · — | Partly | WML | T: command pack (`journalctl`, `wevtutil`, `log show`) | — | None | 4 |
| A permission the person grants the engine (Accessibility, Automation, Full Disk Access, polkit) | — | — | WML | Not stated whether rule 5 allows the engine to ask for one | Every macOS desktop and scripting route needs one | Owner's question, decision 0211 | 5 |

## Data

| Mechanism | Read · change | Describes itself | OS | Place today | Gap | To close it | Rules |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Delimited and sheet files | Row by key, named cell · replace the file | Headings, defined tables | WML | B: file pack, `file`, artifact; delimited writes built, workbook writes not | Workbook writes | Roadmap H3 | 3 |
| JSON | Key path · replace | Structure | WML | B: file pack | — | None | 3 |
| YAML, TOML | Key path · — | Structure | WML | D: read only until a format-preserving editor passes | Writes | S to M | 3 |
| XML | Element · — | XSD, sometimes | WML | D: read only at first | Writes | M | 3 |
| INI files and property lists | Key · key | Structure | WM (L) | N | — | Formats in the file pack, S each | 3 |
| Documents (PDF, word-processor, presentation) | Form field, content control · — | Named fields, where the author made them | WML | N | — | A read-only reader in the file pack, M; writes left out | 3 |
| Media files' metadata | Tag · tag | Tag names | WML | N | — | Formats in the file pack, M | 3 |
| Archives | List of entries · — | Entry names | WML | N; the zip container is read only inside workbooks | — | With the folder mode, M | 7 |
| Embedded databases | Statement · keyed write | Catalog | WML | B: data pack, `sql` | — | None | 3 |
| Database servers | As above | Catalog | WML | B for Postgres behind a feature, on a loopback IP; MySQL and SQL Server researched ([note 23](../research/23-server-database-clients.md)) | Other makes | M each | 3, 4 |
| A universal bridge (ODBC) | — | Through the driver | W mostly | N by design: one client per protocol ([section 307](../spec/3-packs/307-the-data-pack.md)) | — | Leave out | 7 |
| Key-value and document stores | Read by key · keyed write | Sampled shapes, `inferred` | WML | C: a later mode of the data pack | — | M | 3 |
| Search indexes | Query · — | Mappings | WML | T: HTTP pack | — | None | — |
| Object stores | Object · put | Listings | WML | T in section 106, but not reachable: XML answers and signed requests are not designed | Both | Answer formats (above), and an auth provider that signs each request, M | 4 |
| Another program's private store | — | — | WML | R | — | None | 4, 5 |

## Streams and messages

| Mechanism | Read · change | Describes itself | OS | Place today | Gap | To close it | Rules |
| --- | --- | --- | --- | --- | --- | --- | --- |
| A broker: NATS with JetStream | Retained value, bounded wait · keyed publish | Channel lists, schemas | WML | B: stream pack, `stream`, stream; against the stand only | A real server not run | Roadmap J2, J3 | 3 |
| Brokers of other protocols (MQTT, AMQP, Kafka) | As above | Schema registries | WML | D: one protocol interface in the stream pack; MQTT second ([note 22](../research/22-stream-wire-protocol.md)) | Not built | M each | 3 |
| Mail, reading | Message, folder · flag, move | Folder lists | WML | C | — | Request mode, M | 4 |
| Mail, sending | — · submit | — | WML | C, `irreversible` | — | S after reading | 3 |
| Calendars and contacts | Entry · entry | DAV properties | WML | N: XML answers and DAV methods; the systems' own stores are protected | — | After answer formats, M | 4, 5 |
| Feeds | Entry · — | Structure | WML | N: XML | — | After answer formats, S | — |
| Logs and files that grow | Lines after a recorded place · — | No | WML | N: the stream pack reads brokers only | — | A stream mode over a declared file, S to M | 3, 4 |
| Change notifications of the file system | Wait for a change · — | — | WML | N; runs started by an event are not designed | — | After triggers | — |
| Webhooks arriving | — | — | WML | N: nothing listens for another machine (decision 0208) | An outside service cannot deliver | Read from a relay the person already uses, as a stream; or poll. Leave out | 0208 |

## Devices

| Mechanism | Read · change | Describes itself | OS | Place today | Gap | To close it | Rules |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Industrial registers over TCP | Register · register | A published map, as a file; **reads stated** | WML | B: device pack, `device`, device, against the simulator | — | None | 10 |
| Serial lines | Register, console line · the same | As above | WML | D: "later a serial port" | Not built | M | 10 |
| OPC UA | Node · node | Address space; **reads stated** (access levels) | WML | N | — | Device-pack mode, L | 10 |
| USB devices | — | Descriptors only | WML | N | — | Leave out | 5, 10 |
| Bluetooth devices | Characteristic · characteristic | GATT services; **reads stated** (properties) | WML | N | — | New mode, L; leave out | 10 |
| Home-automation hubs | State · service call | Often | WML | T for a JSON HTTP interface; N for their WebSocket and MQTT faces | — | WebSocket mode; MQTT | 10 |
| Printers and scanners | Attributes · job | IPP attributes, scanner capabilities | WML | N: binary and XML answers | — | After answer formats, M | 3 |
| Cameras | — | A SOAP description | WML | N | — | Leave out | 4 |
| Network equipment | Object · object | MIBs (`MAX-ACCESS`), YANG (`config false`); **reads stated** | WML | T for a JSON HTTP interface; N for SNMP and NETCONF | — | Request modes, M to L | 10 |
| Audio and control surfaces | — | No | WML | N | — | Leave out | 10 |
| Local devices found by name (mDNS) | — | Service types | WML | R: a device link refuses a host name | — | Decision 0214, S | 10 |
| Firmware | Image inspected · update by the device's mechanism | — | WML | D: device pack (roadmap K4 ready) | — | Roadmap K4 | 5, 10 |
| A debug probe, memory | — | — | WML | R | — | None | 5 |

## Code as the thing wrapped

| Mechanism | Read · change | Describes itself | OS | Place today | Gap | To close it | Rules |
| --- | --- | --- | --- | --- | --- | --- | --- |
| A library with a C interface | Function · function | Headers: types, not effects | WML | C: bridge, `library` for planning | Memory faults in a helper process | Leave out | 7 |
| A managed assembly or class archive | Method · method | Reflection; no reads stated | WML | C | A runtime to host it | Leave out | 6 |
| A script module in a dynamic language | Function · function | Type stubs, sometimes | WML | T only as a fixed interpreter and script ([section 306](../spec/3-packs/306-the-command-pack.md)) | Whether a script the developer keeps counts | Decision 0214, S | 6 |
| A WebAssembly component | Function · function | WIT; no reads stated | WML | N | — | Leave out; the best-fitting form of a library later | 6 |
| A language server | Symbols, diagnostics · edits it returns | Capabilities; reads stated by method | WML | N | — | JSON-RPC mode over standard streams, M | 7 |
| A debugger adapter | — | Capabilities | WML | R: it attaches to a running program | — | None | 5 |
| A notebook kernel | — | Kernel information | WML | R: it runs code text | — | None | 7 |
| An editor's remote interface | Buffer, state · command | Some publish their interface; some take code | WML | N; those that take code are R | — | Leave out | 7 |

## Others

| Mechanism | Read · change | Describes itself | OS | Place today | Gap | To close it | Rules |
| --- | --- | --- | --- | --- | --- | --- | --- |
| A mobile device attached by cable | Screen elements · the same | Its automation tree | WML | C | — | A pack after the desktop pack, L | 5 |
| Virtual machines and the Linux subsystem on Windows | Query · start, stop | Some hypervisors describe their machines in XML | WML | T: command pack, except the launchers that start a named program, which are R | — | None | 7 |
| A remote desktop session | — | No | WML | R: pixels | — | Decision 0212 | 1, 7 |

## What the map shows

- Of about a hundred mechanisms, fewer than twenty are built on any system, and nearly all of those on Linux only.
- More than twenty describe themselves, and about a dozen of those state which calls only read. That is the property that lets a scout draft and try reads with no person writing them. The design imports four kinds of description today (OpenAPI, an MCP server's tool list, a published register map, a database's catalog), each through its own finding kind and drafting rule.
- The largest families with no place are the operating systems' scripting interfaces, answers that are not JSON, and files that are not tables: folders, documents, property lists, logs.
- Every refusal in the map rests on one of the ten rules or on a design limit. The design limits with a way through are in decision 0214; the questions for the owner are decisions 0211, 0212, 0213, and 0215.
