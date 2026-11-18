# Coverage review, 2026-11-18

One design review round, by [lens 15](../plan/design-review-lenses.md), coverage, which the owner asked for: for every way software on a personal computer can be read or changed from outside itself, does this design have a place for it, and is that place the right one? The owner's direction is [decision 0208](../decisions/0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md): "literally anything their machine can read", on Windows, macOS, and Linux, served on that machine only.

The result is the [coverage map](coverage-map.md), one row per mechanism, and the findings below, most important first. The round wrote documents only. It was run beside the landscape survey of [research note 27](../research/27-the-public-landscape-of-agent-tools.md), and says at the end where the two agree and where they do not.

## Findings

Each finding says what is wrong, where, how serious, whether it is a contradiction, a gap, or a weakness, and where it went.

| # | Finding | Where | Serious | Kind | Sorted to |
| --- | --- | --- | --- | --- | --- |
| 1 | **"A published description becomes tools" is one idea in the kernel's types and several in practice.** The kernel has the parts: a finding whose source is `published_description` and whose level is at most `inferred`, the `DraftingRule` trait from one finding kind to one file kind, open questions in place of guesses, and `untrusted_text`. Each pack then defines its own finding kind and rule: `described_operation` in the HTTP pack (OpenAPI only is built), `upstream_tool` in the MCP pack, a register map in the device pack, `window` and `screen` in the interface packs. More than twenty mechanisms in the map describe themselves and about a dozen state which calls only read (GraphQL, OData, an MCP tool list, a site's tools, scripting dictionaries, type libraries, D-Bus introspection, OPC UA, Bluetooth services, management object trees, published register maps). A kernel shape for a described operation would make each a parser and a lowering | [Section 218](../spec/2-kernel/218-discovery-and-findings.md), [section 402](../spec/4-building-tools/402-the-scout-and-who-does-what.md); `a2m-files` `DraftingRule`; `a2m-pack-http` `openapi.rs` | High | Weakness | Hypothesis H-41 |
| 2 | **The operating systems' scripting interfaces have no place.** Apple events and scripting dictionaries, COM automation and type libraries, D-Bus, WMI, and PowerShell are how applications, and Windows itself, are automated. Section 106 catalogs them under bridge; the command pack refuses a shell and has no fixed pair built. The cheapest honest route is a script the developer keeps in the target, run by a fixed interpreter with its arguments as data; D-Bus is a medium new mode with its crate already in the workspace | [Section 106](../spec/1-product/106-what-it-latches-onto.md), [section 306](../spec/3-packs/306-the-command-pack.md) | High | Gap | [Decision 0214](../decisions/0214-ways-through-the-refusals-that-keep-a-person-from-their-own-machine.md), part 6 (Proposed); open point in section 306; research brief 35 |
| 3 | **The HTTP pack reads only JSON answers, so several kinds said to be "through the HTTP kind" are not.** Object stores answer in XML and need each request signed; feeds, SOAP, calendars and contacts over DAV, printers, scanners, and many devices' web pages answer in XML or binary | [Section 303](../spec/3-packs/303-the-http-pack.md) ("a body that is not JSON fits no schema"); section 106 | High | Contradiction and gap | Section 106 corrected; map row |
| 4 | **Three-system parity is the largest owed item, and one piece fails open.** The desktop pack reads on Linux only; the MCP pack's local program is not built on Windows; the command pack's path check, on any system that is not Unix, resolves the path and refuses nothing, so the refusal section 306 states does not hold on Windows. On macOS the folder of installed applications is believed to be writable by the administrators' group, which the check refuses whatever the group | [Crate design of `a2m-pack-command`](../architecture/crates/a2m-pack-command.md), item 9; `crates/a2m-pack-command/src/safety.rs` | High; a safety gap on Windows | Gap | Crate design corrected to say it passes; decision 0214, part 1; research brief 35 |
| 5 | **Rule five costs the landscape's largest draw, and the documents did not agree on it.** The person's own signed-in browser is refused by rule five, while [decision 0031](../decisions/0031-the-browser-always-runs-with-a-profile-the-engine-owns.md) and section 704 held an extension in the person's profile as "possible later". An application built on a browser runtime, started by the supervisor as section 301 says, would open its own data directory, another program's store | Rule five; decision 0031; [section 704](../spec/7-project/704-readiness.md); [section 301](../spec/3-packs/301-the-web-pack.md) | High | Contradiction | Sections 704 and 301 corrected; [decision 0211](../decisions/0211-whether-the-engine-may-ever-act-in-a-session-the-person-already-has.md) (Proposed) |
| 6 | **"Modify in any way" asks for actions the verified-write model makes unusable.** Most actions are writes of the state they leave and can be read back; the rest (a notification, a print, a key, a send) end on an acknowledgement, honestly, as `irreversible`, and so need a person every run, with no standing approval | [Section 201](../spec/2-kernel/201-archetypes-and-modes.md); [section 212](../spec/2-kernel/212-policy-and-approvals.md) | Medium | Weakness | Hypothesis H-42; [decision 0215](../decisions/0215-whether-a-person-may-pre-approve-an-action-that-leaves-nothing-to-read-back.md) (Proposed) |
| 7 | **The seven archetypes hold, but bridge and request may be one.** Nothing in the map fits none of the seven except what a rule refuses. An out-of-process scripting interface fills the loop as a request does; what would be left to bridge is code inside the host, which this round would leave out of the first release | Hypothesis H-29; section 106 | Medium | Weakness | H-29 note |
| 8 | **Files that are not tables have no place**: folders as collections of entries, growing logs, archives, documents' named fields, media metadata, property lists, INI files. They are most of the files a person's applications keep | [Section 308](../spec/3-packs/308-the-file-pack.md) | Medium | Gap | Map rows |
| 9 | **Loading a target someone else wrote: two documents disagree.** [Decision 0058](../decisions/0058-a-toolkit-for-developers-with-no-catalog-and-no-preferred-agent.md) says a package "they built themselves"; [section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md) designs taking a target written by someone else | Decision 0058; section 502 | Medium | Contradiction | [Decision 0213](../decisions/0213-whether-a-target-someone-else-wrote-may-be-loaded-and-found.md) (Proposed) |
| 10 | **Each refusal added by the safety audits has a way through inside the rules**: the administrators' group treated as the system's own; a declared key pattern beside an action word; `localhost` as a fixed spelling of loopback, never resolved; a device's name resolved once at `target.add`; elevation of a declared program through the system's own prompt | Decisions 0140, 0188, 0195, 0197; section 306 | Medium | Weakness | Decision 0214 (Proposed, design) |
| 11 | Section 106 called procedure-call and document-style services designed; only GraphQL and OData are, gRPC comes after the first release, and SOAP and JSON-RPC have no design. Section 201 listed gRPC reflection under `api` | Sections 106, 201, 303 | Low | Contradiction | Both corrected |
| 12 | Pixel-only applications were "until a later vision mode" in section 102, "not planned" in section 704, and refused in section 106 | Sections 102, 704, 106 | Low | Contradiction | Sections 102 and 704 point to [decision 0212](../decisions/0212-whether-an-application-that-offers-only-pixels-is-ever-reached.md) (Proposed) |
| 13 | Webhooks from outside services cannot arrive: decision 0208 lets nothing listen for another machine, and runs started by an event are not designed | Decision 0208; section 704 | Low | Gap, by an owner rule | Map row; left out |
| 14 | WebSocket, server-sent events, and JSON-RPC over standard streams have no surface, though local applications, language servers, and home-automation hubs speak them | Sections 301, 303, 304 | Medium | Gap | Map rows |

## The questions the brief asked

**Is "a published description becomes tools" one idea or several?** Several, on one base (finding 1). The kernel already holds the rules that matter: a description is a claim at `inferred`, a draft states nothing the system did not, and an open question stands where it did not. What it does not hold is the shape of what a description says about one operation. Each pack invents it, and only the HTTP pack's import is built. The contract would be a kernel-owned finding shape, `described_operation`, lifted from the HTTP pack's: a name and the place in the description it came from; inputs and outputs in the closed subset of JSON Schema the endpoint catalog already uses; what the description states about effect (`safe`, `read_only`, `idempotent`, `destructive`, or nothing); the key, where stated; the description's own example, where given; units, where stated. One kernel drafting rule would write the operation (a read only where the effect is stated safe or read-only, otherwise an open question), and each pack's rule would write only the binding. A new kind of description would be a parser into the shape and a lowering. Whether that is simpler than a finding kind per pack is hypothesis H-41; its test is the GraphQL import, built both ways.

**Do the seven archetypes hold?** Yes, so far (finding 7). Scripting interfaces fill the loop as requests; embedded scripting hosts and libraries as bridge; pixel-only applications would be interface if a rule allowed them. The one candidate for a change is a smaller set, not a larger one, and it is not worth making until a bridge mode exists.

**What does "modify in any way" ask that the verified-write model cannot give?** A write to something with no reported state, done often. "Accepted by the system, not read back" (decisions 0161, 0193, 0196) is enough as evidence: it is honest, and resume does not repeat it. It is not enough as a way of working, because every such call needs a person (finding 6). Also outside the model, and refused by rule: changing a running program's memory, writing through a debugger, and any change past a protection.

**Where does a rule stand in the way?**

| Rule | What it costs | Way through inside the rule | Question for the owner |
| --- | --- | --- | --- |
| 5: the engine-owned profile | Every site and browser-runtime application needs a sign-in in the engine's profile; one-session applications may sign the person out | Sign in once, in a visible window of the engine's profile; start a browser-runtime application with a data directory the engine owns | Decision 0211: an extension in the person's profile; an application's own data; permissions the person grants the engine |
| 1 and 7: pixels | Remote desktops, and programs with no accessibility data and no other surface | Use the application's other surfaces; a screenshot at build time only | Decision 0212 |
| 9: no catalog | Every person builds every target; the first hour is longest for the most-used applications | Load any target the person chooses under section 502's rules | Decision 0213 |
| The audits' refusals (design limits, not rules) | macOS applications, keys with action words, `localhost`, device names, administrator changes | Decision 0214, in six parts | None: design decisions |
| 3, through section 212 | Frequent actions with nothing to read back | Bind actions as state writes (H-42) | Decision 0215 |

**The smallest set of additions that would let the product say "anything your computer can reach", in order.** The honest claim is narrower than "anything": *anything your computer can reach through an interface it offers for reading or changing it, with a named stop where it offers none.*

| # | Addition | Size |
| --- | --- | --- |
| 1 | Three-system parity for what is built: the desktop pack's Windows and macOS back ends, the MCP pack's local program on Windows, the command pack's path check on Windows and its answer for macOS (decision 0214, part 1). Owed by decision 0208 | L, L, S to M, M |
| 2 | A proxy the person names (`network.proxy`), owed by decision 0208 | S |
| 3 | A script the developer keeps, as a fixed pair (decision 0214, part 6): the scripting interfaces and PowerShell on all three systems | S |
| 4 | Answers that are not JSON in the HTTP pack, and an HTTP surface on a local socket or pipe | M, S to M |
| 5 | The shared described-operation shape (H-41), then the designed GraphQL and OData imports, then a D-Bus mode | M, then S each, then M |
| 6 | In the file pack: a folder mode, property lists and INI, a growing file read from a recorded place | M, S, S to M |
| 7 | A request mode over WebSocket and standard streams for JSON-RPC (language servers, local applications' interfaces, hubs) | M |
| 8 | Mail, reading and sending | M |

**What the first release should not try to cover:** pixels and remote desktops; the person's own browser profile, unless the owner answers decision 0211 otherwise; mobile devices; terminal screens and programs with a prompt; embedded scripting hosts and add-ins; libraries in any language and WebAssembly components; gRPC and SOAP; ODBC; Bluetooth, USB, OPC UA, SNMP, NETCONF, cameras, audio surfaces; webhooks from outside; remote shells; debuggers and notebook kernels; anything started by an event.

## Against the landscape survey

[Research note 27](../research/27-the-public-landscape-of-agent-tools.md) and this round were done apart and agree on the main points: the person's own browser, the scripting interfaces, ready-made integrations, and pixels are the largest gaps, and three of them meet a rule. Its three owner questions are asked here: its first in decision 0211 (question 1), its second in decision 0213 (question 2), its third in decision 0212 (question 3). Its fourth, that the refusal of a general shell stands, this round agrees with and asks nothing about.

Where this round differs:

- **The scripting interfaces as a pack.** Note 27's candidate (1) is "a bridge pack ... a pack per system". This round puts them under the request archetype (finding 7), and proposes the fixed-pair script first (decision 0214, part 6), because it reaches all three systems at small size with rules already written; a pack per system, reading the dictionaries and type libraries as descriptions, follows.
- **gRPC is not a small addition.** Note 27's finding 7 calls a gRPC reflection reader "a small addition" and cites section 106 as listing procedure-call services as designed. Section 303 puts gRPC after the first release because it needs HTTP/2, protobuf bodies, and a stand; this round sizes it medium to large and corrected section 106 (finding 11).
- **OPC UA and mobile** (note 27's candidates (2) and (4)) are in the map, both large, and left out of the first release.
- Note 27's candidates (5) and (6), a one-command first result and a page on what the product has, are outside this lens and not judged here.

## What was fixed in the documents

- [Section 106](../spec/1-product/106-what-it-latches-onto.md): the stack row for language runtimes; the state of desktop applications, web services, an existing MCP server, and object storage; a pointer to the coverage map; "cover the rest" made "cover much of the rest".
- [Section 201](../spec/2-kernel/201-archetypes-and-modes.md): gRPC reflection removed from `api`.
- [Section 301](../spec/3-packs/301-the-web-pack.md): an application built on a browser runtime starts with a data directory the engine owns, by rule five.
- [Section 102](../spec/1-product/102-requirements-and-guarantees.md) and [section 704](../spec/7-project/704-readiness.md): pixels and the extension point at the Proposed records.
- [Section 306](../spec/3-packs/306-the-command-pack.md): two open points; the [command pack's design](../architecture/crates/a2m-pack-command.md): its path check passes everything on Windows.
- The [hypothesis register](../hypotheses/README.md): H-41, H-42, and a note on H-29. [Open research](../research/11-open-research.md): brief 35. The [lenses](../plan/design-review-lenses.md): lens 15. The [documentation index](../README.md): this file and the map.

## Deferred, and why

- The kernel contract of finding 1: it needs the evidence of H-41 before a record.
- Every addition in the table above: each is a package for the [roadmap](../plan/roadmap.md), which the lead orders. The round adds no row to it.
- Every claim marked "believed" in the map: research brief 35.

## Honest return

The round found more than a usual one, because the lens was new and the design had never been read against a list of mechanisms. Most of what it found is gaps, not errors; the corrections are few and small. The next gain is from building: items 1 to 3 above, and the test of H-41 on the GraphQL import, will say more than another reading.

## Not read

The pack sections 307 to 310 were read in their scope and refusal tables only; sections 203 to 217 and 219 to 226 beyond what is cited; the decisions beyond those cited; research notes 01 to 25 beyond notes 19, 22, 23, and 26 in part; the crate designs beyond those of the command, MCP, device, file, and desktop packs in part. Nothing was run but the documentation audit, and nothing on Windows or macOS was checked.
