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
| Wraps what the person's own machine can reach, on Windows, macOS, and Linux ([decision 0208](docs/decisions/0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md)) | Find what is on the network the machine is on. The scout starts from an address a person names; network awareness is decided ([decision 0222](docs/decisions/0222-the-product-is-aware-of-the-network-its-machine-is-on.md)) and being designed. The owner decided how far looking goes ([decision 0242](docs/decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md), points 6 to 8): no asking of many hosts or many ports, nothing sent on a network the person has not named beyond the addresses named in a target, and findings kept for the run unless the person saved the network | Not yet |
| Web applications, through the page in a browser the engine owns, and through requests the page itself makes: reads and verified writes | Use the person's everyday browser profile or another program's stored sign-ins | Rule 5, which the owner confirmed ([decision 0242](docs/decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md), [0211](docs/decisions/0211-whether-the-engine-may-ever-act-in-a-session-the-person-already-has.md)). A permission the person grants the engine in the system's own settings (accessibility, automation) is within rule 5 |
| HTTP services: reads and verified writes with JSON; answers in XML, plain text, or bytes are read by declared paths and patterns; an imported OpenAPI description | Send request bodies that are neither JSON nor form-encoded; save a binary answer to a file; GraphQL, OData, gRPC, SOAP | Not yet |
| Existing MCP servers, with verification added | Start a local MCP server program on Windows | Not yet |
| Command-line programs, as declared commands with typed arguments | Offer a shell, or run text an agent wrote | Choice ([section 213](docs/spec/2-kernel/213-effects-hazards-and-consent.md)): what such text would do cannot be declared, approved, or verified |
| Runs a script the developer keeps in the target, as a fixed pair with a declared interpreter, checked by digest at every start (proven on Linux; written for Windows and macOS, not yet run there) | Start a program with administrator rights | Not yet |
| Databases (an embedded database file; PostgreSQL in a build with that feature), files (delimited text, JSON, workbooks), streams (NATS), devices (against a simulator) | Folders, growing logs, property lists; other brokers; other databases; firmware updates | Not yet |
| Desktop applications through the accessibility interface, on Linux: reads, and verified writes that set fields, press the save action, and read back what the window shows is stored. An act never lands on another record or under a window no screen names | Desktop reads on Windows and macOS (both written, neither yet run; on macOS the person first allows the program that started the engine under Privacy and Security, Accessibility); desktop writes on Windows and macOS; number fields; creating and deleting records; starting an application | Not yet |
| | Click by position on a picture of the screen | Rules 1, 3, and 7: reading a picture takes a learned model (rule 1), a click by position cannot be proven against the element it was meant for (rule 3), and a picture that differs from the recorded one is a variant with no rule (rule 7). The owner decided that such applications are not reached at run time, and that the scout may keep a screenshot as build-time evidence, which is not built yet ([decision 0242](docs/decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md), [0212](docs/decisions/0212-whether-an-application-that-offers-only-pixels-is-ever-reached.md)) |
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
| Runs a binding in shadow beside the verified one on a live read, compares the two answers, reports the comparisons (`shadow report`), and lets a person promote the shadow binding after a run of exact matches | Run a shadow of a write | Not yet |
| Answers when it is asked: a call, its progress, and a run's events as a stream. The ledger and the scheduler can keep a watch (a published read repeated on a schedule its person confirmed) and record what changed, as digests only ([decision 0247](docs/decisions/0247-a-watch-keeps-only-digests-and-its-due-times-come-from-the-stores-clock.md)) | Let an agent ask for a watch, or tell a client that something changed: the actions are not wired. Start a run because something changed: a change is reported and never acted on | Not yet for the first two; a choice for the third ([decision 0234](docs/decisions/0234-a-watch-is-a-read-repeated-on-a-schedule-a-person-confirmed-and-a-change-is-reported-never-acted-on.md)) |
| Runs a call when it is made | Run at a set time, or start a run because something happened | Not yet |
| Runs a pipeline over many records of one target, with a checkpoint for each record | Join operations of two targets in one pipeline; undo the first when the second fails; carry on past a record the system refused | Not yet for the first; choice for the second and third (safe before easy) |
| Takes a file a pipeline reads, from a folder the person allows | Hand a file back as a result, or send one to a system | Not yet |
| Takes and gives times in UTC or at a fixed offset | Take a named time zone with daylight saving | Not yet |

## People and agents

| It does | It does not, today | Kind |
| --- | --- | --- |
| Serves tools to any client that speaks the protocol, and prefers none | Act as an agent or an agent framework: no planning, memory, or conversation | Choice ([section 103](docs/spec/1-product/103-design-principles.md)) |
| Holds a write until a person at the machine approves it, and binds the approval to that exact change | Let an agent approve, publish, or sign in; offer any command, tool, or route that approves | Choice in the design, resting on rule 2 |
| Proves a person was present with the operating system's own prompt (proven on Linux against a real polkit) | Prove presence on macOS and Windows: the prompts are written and no person has run them. Approve anything from a container or a remote shell | Not yet, for the first; by design, for the second |
| Shows exactly what will change before approval | Answer for whether the person read it | Choice: nothing can |
| Assumes the calling agent may be steered: no input changes where a request goes or which file is read | Answer for what the agent decides, or whether it believes what a system told it | Choice: that is the agent's host |
| | Give standing approval to an action that leaves nothing to read back, so each such call needs a person | Not yet. The owner allowed it narrowly ([decision 0242](docs/decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md), [0215](docs/decisions/0215-whether-a-person-may-pre-approve-an-action-that-leaves-nothing-to-read-back.md)): only for an operation whose declared hazard is none and which is `irreversible` solely because nothing can be read back, only on one machine, and revocable. It is built after measuring how many actions it covers, and not built if that number is small |
| Shows a waiting run on the approval page | Tell the person anywhere else that a run waits: no notification is built | Not yet |
| Serves tools through MCP (tools, progress, cancellation, discovery), REST, the command line, and the Python and Node clients | Serve MCP resources, subscriptions, prompts, tasks, completions, or elicitation | Not yet |
| | Ask the client's model to generate anything (sampling) | Rule 1 in a run; a choice at build time |
| Shows approvals and sign-ins on the machine itself | Let a person approve from another device | Owner decision [0208](docs/decisions/0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md) |
| | Speak any language but English in its own words | Not yet |

## Secrets and sign-in

| It does | It does not, today | Kind |
| --- | --- | --- |
| Keeps tokens, passwords, and cookies in a sealed vault and out of every input, result, log, trace, capture, fixture, and message | Hand a secret to a caller, for any reason | Rule 4 |
| Lets an agent ask for a sign-in that the person carries out on the engine's own page | Create accounts, choose or rotate passwords, recover access, or answer a second factor by itself | Rule 5 for the last; choice for the rest |
| Uses the operating system's key store on macOS and Windows | Use one on Linux: this build takes a passphrase there ([decision 0123](docs/decisions/0123-the-linux-secret-service-back-end-is-left-out-until-its-store-crate-is-maintained.md)) | Not yet |
| | Sign in through a web application's own pages, by form or by single sign-on, with or without a second factor | Not yet: the web pack's sign-in providers are not built |
| | Use the system's integrated sign-in with the current logon, or a smart card or a passkey with the person present | Not yet. Allowed, through the holder and never read ([decision 0242](docs/decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md), [0232](docs/decisions/0232-whether-a-credential-the-machine-holds-for-the-person-may-be-used-through-its-holder.md)) |
| | Use an SSH or signing agent | Owner decision [decision 0242](docs/decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md): not in the first release |
| | Use another program's stored credentials: a cloud tool's credential file, a credential helper, a password manager | Rule 5 |
| | Sign each request as cloud providers ask | Not yet |

## The machine and the network

| It does | It does not, today | Kind |
| --- | --- | --- |
| Runs on the person's machine and listens only there: a socket or pipe of the person's own, and the approval page on the loopback address | Listen for another machine; run as a shared server | Owner decisions [0169](docs/decisions/0169-the-first-release-is-for-one-machine.md) and 0208, for the first release |
| Protects its own files and channel from other ordinary users and from callers | Defend against the machine's administrator, a compromised operating system, or malware already running as the person | Choice: an administrator can replace the program itself |
| Refuses to start a declared program that another user could have replaced (proven on Linux; the Windows check, from owners and access lists, is written and has not yet run there) | Read access lists beyond the mode on Linux and macOS; take a program owned by another administrator | Not yet |
| Sends nothing home: no usage data, crash reports, or update checks unless a person switches them on | Update itself, or tell the person a new version exists | Choice (a convention in AGENTS.md) |
| Refuses to send a credential over an unencrypted link to another machine | Reach the outside through a proxy. One the person names is owed by decision 0208 | Not yet |
| Reports what it meets on its way as a plain finding: a link that is not encrypted, a certificate that does not verify | Look for weaknesses. It is not a vulnerability scanner, an intrusion detector, a firewall, or a monitor | Choice. Trying credentials nobody stored, or exploiting anything, is rule 5 |
| Runs inside one command for a build server or a scheduled job | Approve a write where no person is present, or keep a Linux engine's sessions without a person giving the passphrase | By design for the first; not yet for the second |
| Reaches another machine through what that machine already serves | Receive an outside service's webhook; reach the engine on the person's other machine | Owner decisions [0208](docs/decisions/0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md) and 0242 ([0233](docs/decisions/0233-whether-being-told-by-another-machine-is-serving-it.md)): not in the first release |
| Reaches the system's own programs one declared command at a time | Treat the machine itself as a target the scout starts from; list, move, or copy a folder's files | Not yet |
| Judges in one way, everywhere, whether a plain link stays on this machine: only a loopback address written as an IP address, read from the address alone ([decision 0240](docs/decisions/0240-one-typed-reading-of-where-an-address-is-and-one-question-of-a-plain-link.md)) | Resolve a name to decide where an address is; decide anything yet from the kind of place a destination's address is | Choice (decisions 0188 and 0192), for the first; not yet, for the second |
| Reads what the machine already knows of its own networks from the operating system, sending nothing: interfaces, routes, name servers, the neighbour table, listening sockets, the proxy setting, and the kind of place each address is. Proven on Linux; written for Windows and macOS and not yet run there ([decision 0243](docs/decisions/0243-the-machine-reads-its-own-network-state-and-sends-nothing.md)) | Serve these as tools; read the wireless network's name; read listening sockets or the proxy on macOS; recognize which network it is on | Not yet |

## Building tools

| It does | It does not, today | Kind |
| --- | --- | --- |
| Scouts a system from an address a person gives, drafts the reads that follow by rule, tries them, records a scrubbed fixture, runs gates, and promotes a binding on that evidence | Draft writes. A write is the developer's judgment and is written by hand. Where a system itself states a write's endpoint, key, and read-back, drafting the mechanics and leaving the judgment as open questions is an open proposal | Choice ([section 402](docs/spec/4-building-tools/402-the-scout-and-who-does-what.md)) |
| Runs two of the twelve publish gates (the schema and lint gate, and the offline replay) | Run the other ten | Not yet |
| Publishes a version only when a person reads the page and passes the presence prompt | Decide what a tool means: names, units, which record is the same record, an operation's effect and hazard. That is the developer's | Choice |
| Keeps a target as a folder of plain files the developer owns | Publish or host a catalog of targets for real applications | Rule 9 |
| | Load a target someone else wrote | Not yet. Allowed, under the rules of [section 502](docs/spec/5-security/502-defending-against-a-manipulated-agent.md): it is compiled, linted, and gated as any target, and its evidence is never trusted ([decision 0242](docs/decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md), [0213](docs/decisions/0213-whether-a-target-someone-else-wrote-may-be-loaded-and-found.md)) |
| Prints the configuration block an MCP client needs (`connect`), and writes it into a client's file from a profile the person keeps as data, after showing the change | Ship a profile for any particular client; install plug-in packs | Choice for the first (no client is special); not yet for the second |
| | Learn a tool from a person doing the task once | Not yet |
| Publishes a version | Tell the person a tool has drifted before it is called; compare, roll back, move, or retire a version | Not yet |

## Configuration

| It does | It does not, today | Kind |
| --- | --- | --- |
| Takes settings from eight layers, refuses an unknown or misspelt key by name, and shows every value and the layer that set it | Act on every key it declares: 55 of the 95 kernel keys are read by no code, and a value for one is accepted without effect (decision [0236](docs/decisions/0236-a-declared-setting-whose-feature-is-not-built-is-refused-by-name.md) has such a key refused by name; not built) | Not yet |
| Applies the project file of the folder the engine started in | Apply each project's own file to that project's requests (decision [0237](docs/decisions/0237-one-statement-of-what-is-said-at-which-scope-by-which-means-and-by-whom.md); not built) | Not yet |
| Lets a preset, the user's file, the project's file, a variable, or a flag tighten a protected setting | Hold that line everywhere: today the variable that moves the engine's home also moves the installation's file into a folder the person's own programs can write, and that file can loosen a protected setting. A known fault; decision [0237](docs/decisions/0237-one-statement-of-what-is-said-at-which-scope-by-which-means-and-by-whom.md) closes it and is not built | Not yet |
| Evaluates policy by tool, module, target, environment, toolset, records, effect, hazard, and whether the caller is an agent | Let a person on one machine write a policy of their own: no pre-approval rule, quota, or grant can be made | Not yet |
| Keeps traces, captures, runs, and the audit on the machine | Delete any of them when their retention ends | Not yet |
| Changes a setting at the next start of the engine | Change one while it runs, or through a command | Not yet |
| | Tell one client connection from another in settings or policy: one ceiling holds for every agent | Not yet. Knowing a client's make is barred by owner decision 0058; a connection the person names is not |
| | Run a hook of the person's own before or after a run | Not yet |
| | Let a person's own code take part in proving a write, as a field type's way of comparing | Rules 3 and 6. Owner decision [decision 0242](docs/decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md) ([0238](docs/decisions/0238-whether-a-persons-own-code-may-take-part-in-proving-a-write.md)): not in the first release |
| Names itself in what it sends, as `anything2mcp` and its version | Let a person add to that name, or replace it | Not yet, for adding, which is allowed. Replacing is not allowed in the first release (owner decision [decision 0242](docs/decisions/0242-the-owner-takes-the-recommended-answer-to-every-open-question.md), [0239](docs/decisions/0239-whether-a-person-may-replace-how-the-product-names-itself-to-a-system.md); rule 5) |

No setting can switch off what the ten rules require: there is no key for a model in a run, for skipping the record and read-back of a write, for the redactor, or for stealth. The [configurability review](docs/reviews/configurability-review-2026-11-18.md) lists the fixed frame in full.

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
