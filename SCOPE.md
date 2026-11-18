# Scope: what Anything2MCP does, and what it does not

This page shows both sides: what the product does and answers for, and what it does not. It was last checked against the code and the decisions on 2026-11-18.

**This page describes. It does not rule.** A line under "does not" says where the product stands today and why. It is never by itself a reason to turn an idea down, and it changes by being edited when the product grows ([decision 0230](docs/decisions/0230-the-scope-is-written-down-on-both-sides-and-describes-without-ruling.md)). The only things that bind are the ten rules in [AGENTS.md](AGENTS.md) and the owner's decisions.

Every "does not" carries its kind:

| Kind | Meaning | Who changes it |
| --- | --- | --- |
| **Rule** | It follows from one of the ten rules or an owner decision, named beside it | The owner |
| **Choice** | A design choice in force today | Any agent, with evidence and a decision record |
| **Not yet** | Nothing stands against it. It is not built, or not proven | Whoever builds it |

"Built" below means proven by tests against a test stand. Nothing has been tried against a real system yet, and nothing is published to a package registry.

## What it reaches

| It does | It does not, today | Kind |
| --- | --- | --- |
| Wraps what the person's own machine can reach, on Windows, macOS, and Linux ([decision 0208](docs/decisions/0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md)) | Find what is on the network the machine is on. The scout starts from an address a person names; network awareness is decided ([decision 0222](docs/decisions/0222-the-product-is-aware-of-the-network-its-machine-is-on.md)) and being designed | Not yet |
| Web applications, through the page in a browser the engine owns, and through requests the page itself makes: reads and verified writes | Use the person's everyday browser profile or another program's stored sign-ins | Rule 5. Question open for the owner: [0211](docs/decisions/0211-whether-the-engine-may-ever-act-in-a-session-the-person-already-has.md) |
| HTTP services: reads and verified writes with JSON; answers in XML, plain text, or bytes are read by declared paths and patterns; an imported OpenAPI description | Send request bodies that are neither JSON nor form-encoded; save a binary answer to a file; GraphQL, OData, gRPC, SOAP | Not yet |
| Existing MCP servers, with verification added | Start a local MCP server program on Windows | Not yet |
| Command-line programs, as declared commands with typed arguments | Offer a shell, or run text an agent wrote | Choice ([section 213](docs/spec/2-kernel/213-effects-hazards-and-consent.md)): what such text would do cannot be declared, approved, or verified |
| Runs a script the developer keeps in the target, as a fixed pair with a declared interpreter, checked by digest at every start (proven on Linux; written for Windows and macOS, not yet run there) | Start a program with administrator rights | Not yet |
| Databases (an embedded database file; PostgreSQL in a build with that feature), files (delimited text, JSON, workbooks), streams (NATS), devices (against a simulator) | Folders, growing logs, property lists; other brokers; other databases; firmware updates | Not yet |
| Desktop applications through the accessibility interface, on Linux: reads, and verified writes that set fields, press the save action, and read back what the window shows is stored | Desktop reads on Windows (written, not yet run) and on macOS; desktop writes on Windows and macOS; number fields; creating and deleting records; starting an application | Not yet |
| | Click by position on a picture of the screen | Rules 1, 3, and 7: reading a picture takes a learned model (rule 1), a click by position cannot be proven against the element it was meant for (rule 3), and a picture that differs from the recorded one is a variant with no rule (rule 7). Whether a fixed, non-learned recognizer may ever read a screen is a question open for the owner: [0212](docs/decisions/0212-whether-an-application-that-offers-only-pixels-is-ever-reached.md) |
| | Applications' own scripting interfaces (Apple events, COM, D-Bus); mobile devices | Not yet |
| | Get past a sign-in, a bot defence, a licence check, or a device's protection | Rule 5 |

The full catalog, kind by kind, is the [coverage map](docs/reviews/coverage-map.md).

## What happens when a tool runs

| It does | It does not, today | Kind |
| --- | --- | --- |
| Runs a compiled plan. The same input against the same system takes the same steps | Call a model, or improvise when a page or an answer has changed. It stops with a named failure | Rule 1 |
| Stops when something is not as described | Guess at an unknown control, platform, or response | Rule 7 |
| Records a write before making it, reads the value back, and compares. A lost answer is reconciled before anything is sent again | Call a write verified when nothing can be read back. It ends as "accepted by the system, not read back" | Rule 3 |
| Marks text that came from a wrapped system as untrusted data at every door | Filter or rewrite what a system said | Choice: a filter would be a guess and would hide data a person needs |
| Keeps to declared rates and retries nothing a declaration does not allow | Repair a tool at run time when the system changed. Redrafting is a build-time act | Rule 1 |
| Runs a binding in shadow beside the verified one and compares (built in the engine, for reads) | Offer that in the product: the actions that report and promote are being wired. Shadow of a write | Not yet |

## People and agents

| It does | It does not, today | Kind |
| --- | --- | --- |
| Serves tools to any client that speaks the protocol, and prefers none | Act as an agent or an agent framework: no planning, memory, or conversation | Choice ([section 103](docs/spec/1-product/103-design-principles.md)) |
| Holds a write until a person at the machine approves it, and binds the approval to that exact change | Let an agent approve, publish, or sign in; offer any command, tool, or route that approves | Choice in the design, resting on rule 2 |
| Proves a person was present with the operating system's own prompt (proven on Linux against a real polkit) | Prove presence on macOS and Windows: the prompts are written and no person has run them. Approve anything from a container or a remote shell | Not yet, for the first; by design, for the second |
| Shows exactly what will change before approval | Answer for whether the person read it | Choice: nothing can |
| Assumes the calling agent may be steered: no input changes where a request goes or which file is read | Answer for what the agent decides, or whether it believes what a system told it | Choice: that is the agent's host |
| | Give standing approval to an action that leaves nothing to read back, so each such call needs a person | Rule 2. Question open for the owner: [0215](docs/decisions/0215-whether-a-person-may-pre-approve-an-action-that-leaves-nothing-to-read-back.md) |

## Secrets and sign-in

| It does | It does not, today | Kind |
| --- | --- | --- |
| Keeps tokens, passwords, and cookies in a sealed vault and out of every input, result, log, trace, capture, fixture, and message | Hand a secret to a caller, for any reason | Rule 4 |
| Lets an agent ask for a sign-in that the person carries out on the engine's own page | Create accounts, choose or rotate passwords, recover access, or answer a second factor by itself | Rule 5 for the last; choice for the rest |
| Uses the operating system's key store on macOS and Windows | Use one on Linux: this build takes a passphrase there ([decision 0123](docs/decisions/0123-the-linux-secret-service-back-end-is-left-out-until-its-store-crate-is-maintained.md)) | Not yet |
| | Sign in through a web form with a second factor (the provider is not built) | Not yet |

## The machine and the network

| It does | It does not, today | Kind |
| --- | --- | --- |
| Runs on the person's machine and listens only there: a socket or pipe of the person's own, and the approval page on the loopback address | Listen for another machine; run as a shared server | Owner decisions [0169](docs/decisions/0169-the-first-release-is-for-one-machine.md) and 0208, for the first release |
| Protects its own files and channel from other ordinary users and from callers | Defend against the machine's administrator, a compromised operating system, or malware already running as the person | Choice: an administrator can replace the program itself |
| Refuses to start a declared program that another user could have replaced (proven on Linux; the Windows check, from owners and access lists, is written and has not yet run there) | Read access lists beyond the mode on Linux and macOS; take a program owned by another administrator | Not yet |
| Sends nothing home: no usage data, crash reports, or update checks unless a person switches them on | Update itself, or tell the person a new version exists | Choice (a convention in AGENTS.md) |
| Refuses to send a credential over an unencrypted link to another machine | Reach the outside through a proxy. One the person names is owed by decision 0208 | Not yet |
| Reports what it meets on its way as a plain finding: a link that is not encrypted, a certificate that does not verify | Look for weaknesses. It is not a vulnerability scanner, an intrusion detector, a firewall, or a monitor | Choice. Trying credentials nobody stored, or exploiting anything, is rule 5 |

## Building tools

| It does | It does not, today | Kind |
| --- | --- | --- |
| Scouts a system from an address a person gives, drafts the reads that follow by rule, tries them, records a scrubbed fixture, runs gates, and promotes a binding on that evidence | Draft writes. A write is the developer's judgment and is written by hand | Choice ([section 402](docs/spec/4-building-tools/402-the-scout-and-who-does-what.md)) |
| Runs two of the twelve publish gates (the schema and lint gate, and the offline replay) | Run the other ten | Not yet |
| Publishes a version only when a person reads the page and passes the presence prompt | Decide what a tool means: names, units, which record is the same record, an operation's effect and hazard. That is the developer's | Choice |
| Keeps a target as a folder of plain files the developer owns | Publish or host a catalog of targets for real applications | Rule 9 |
| | Load a target someone else wrote | Rule 9 and decision 0058. Question open for the owner: [0213](docs/decisions/0213-whether-a-target-someone-else-wrote-may-be-loaded-and-found.md) |
| | Write an MCP client's configuration (`connect`, being built); install plug-in packs | Not yet |

## What it answers for, and what stays with others

| It answers for | It does not answer for | Who does |
| --- | --- | --- |
| That what it wrote is what the system then holds, or an honest statement that this could not be read back | Whether the wrapped system is right: its rules, its bugs, its data | The system's owner |
| Working only through access the person already has, and stopping at a block | Whether the person may use, inspect, or wrap a system, or look at a network | The person |
| Noticing that a system changed, and stopping | Keeping a tool working when the system changes | The developer |
| Never sending an irreversible operation twice, and saying it is irreversible before approval | Undoing what a system did | The person and the system's owner |
| A record of what it did and proved, kept on the person's machine | Being the record of the business: it is not a backup, an archive, or a copy of the system's data | The system's owner |
| A declared hazard and range, and a person's approval, on anything that can hurt someone or damage equipment | Being the only protection there. It is not a safety system | The device's maker and the person |
| Pinning other people's code by path and digest and holding it to the contract at the boundary | What that code does inside its own process: a plug-in, a kept script, a local program, an upstream server | Whoever wrote it, and the person who switched it on |
| Keeping data on the machine and giving an audit trail of its own acts | Whether the person's use complies with rules about personal data, finance, health, or records | The person and their organization |
| Treating what a network or a system announces as a claim, never a fact | Facts it cannot see: the machine's clock, a forged announcement, a name that resolves elsewhere tomorrow | The person and the network's owner |
| Stopping by name when a system refuses or is down | The wrapped system's availability, limits, and bills | The system's owner and the person |

All of the lines in this last table are **choices** unless a rule is named elsewhere on this page. They say where responsibility lies today. They are not a fence around what the product may become.

## Keeping this page true

When a change makes a line here untrue, the line is edited in the same change. When this page and another document disagree, one of them is out of date. An idea that a line here seems to rule out is weighed by its kind: a rule goes to the owner as a Proposed record; a choice or a "not yet" is open.
