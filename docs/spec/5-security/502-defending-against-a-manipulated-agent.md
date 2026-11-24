# 502. Defending against a manipulated agent

Agents read text they did not write: records, notes, pages, files. Any of it can contain instructions aimed at the agent. Anything2MCP therefore assumes that the agent calling a tool, and the agent building one, may at some moment be acting for someone else. This section collects what the design does about that, and adds the rules that were missing. Built today: partly; [section 704](../7-project/704-readiness.md) gives the detail.

The aim is simple to state: a manipulated agent should be able to do nothing through Anything2MCP that the person could not have been asked to approve, and should not be able to make Anything2MCP reach, read, or send anything its tools do not openly declare.

## Inputs cannot change where a request goes

Operation inputs are filled into request templates. A crafted input must not be able to redirect the request.

- **Every placeholder has a position, and is encoded for it.** A path segment, a query value, a header value, and a value in a body are each encoded by their own rule. The compiler parses every template and assigns the positions; nothing is assembled by joining strings.
- **A path placeholder cannot contain a separator or a dot segment.** An input of `../admin` is a validation error, not a different path.
- **Scheme, host, and port never come from an input.** They come from the surface.
- **An input that reads like an expression is a value.** Nothing evaluates an input: `${input.x}` given as an identifier is sent as that text, encoded for its position, and a number written as `1 + 2` is refused as not a number (`crates/anything2mcp/tests/faults.rs`).
- **Redirects are followed only within declared surfaces.** Anything else stops with `policy` and the code `surface_not_declared`.
- **Inputs that are themselves addresses** use a `url` field type that accepts only destinations the operation declared. On a server, private and loopback addresses are refused unless declared.
- **A credential never crosses an unencrypted link to another machine.** The HTTP, MCP, and data packs refuse, unsent, a request that would carry a credential over a plain link to an address that is not a loopback address. Only the target's own surface can accept it, in words a reviewer sees and the lint repeats; no input, setting, or variable can ([decision 0188](../../decisions/0188-credentials-over-plain-links-drafted-workbooks-and-published-device-maps.md)).

## Inputs cannot make Anything2MCP read arbitrary files

Sources and uploads take files. A manipulated agent must not be able to name any file on the machine and have it sent to a system.

| Where | Rule |
| --- | --- |
| On a server | A caller never passes a path. It uploads content, which becomes a blob, and passes the blob's reference |
| On one machine | A file is read only from allowed roots: the project folder and folders the person has added. Links are resolved before the check. A pack never resolves a path: it receives a checked path, which only this check, in `a2m-provider`, can make. A project folder is one that holds a project file and is no program's own folder: not the engine's, not the home folder or a hidden folder of it, not a folder the operating system gives programs for their data. A source file is read only from the project's `sources` folder |
| Everywhere | Size limits; the content must be the type it claims, so a renamed file is refused; a file is read once and stored as a blob, so what was previewed is what is uploaded |

One input may name an entry inside a folder the target declares, as a row's key names a row ([decision 0246](../../decisions/0246-the-file-pack-reads-folders-settings-files-and-growing-files.md)). Each part of the name is one name of at most 255 bytes, with no separator, no `..`, and no drive. A read may name an entry as many folders down as the declared depth allows, with `/` between the parts, and reaches none of them through a link; a write names an entry directly in the folder. An input never names a path outside the folder.

## Downloads are data

- A downloaded file goes to a scoped store and comes back as a resource link, never inline.
- Its name is sanitized, its size is limited, and nothing opens or runs it.

## Nothing from the system becomes an instruction

- Tool names and descriptions are written by the target's author and reviewed by a person. Text captured from the system never flows into them unreviewed.
- Results are structured. Free text from the system sits only in fields marked untrusted, limited in length and stripped of markup.
- A validation message from the system, returned with a `rejected` result, is such a field. A driver hands it over apart from the failure's own sentence, bounded and redacted, and every door shows it marked as the system's words, never joined into the product's message ([decision 0162](../../decisions/0162-the-provider-contracts-second-pass-what-a-step-is-handed-and-hands-back.md)), with every character that can reorder, hide, or restyle shown text written as its escape: control characters and terminal escapes, bidirectional marks and isolates, invisible format characters, and line and paragraph separators ([decision 0193](../../decisions/0193-the-core-the-contract-the-compiler-and-the-scheduler-after-the-second-pass.md)).
- Snapshots and captures given to a building agent mark the system's text the same way.
- A tool's whole definition is bounded text, not only its description: the names, descriptions, defaults, and enum labels in its input and output schemas follow the same rules, and the compiler refuses what breaks them ([decision 0325](../../decisions/0325-what-a-browser-can-reach-and-what-a-tools-definition-may-hold.md)). A length bound limits what a description can hold; it does not prove the text harmless. The reviewer, shown every changed description first, and the linter are the barriers, and neither is complete.

## Reaching the engine through a browser

A web page can make a browser send requests to a loopback address, and can make a name resolve to one after the page has loaded. Nothing in the engine is reached by a browser except two listeners: the approval and sign-in listener, which answers only at the one-time code in its address and checks `Host` and `Origin` ([section 503](503-approval-and-sign-in-surfaces.md)), and local HTTP where it is switched on, which is held to the rules of [section 222](../2-kernel/222-front-doors-and-the-local-engine.md). A check of `Host` and `Origin` is the second barrier. The first is that the page holds no credential ([decision 0325](../../decisions/0325-what-a-browser-can-reach-and-what-a-tools-definition-may-hold.md), [research note 39](../../research/39-mcp-transport-and-injection-threats.md)).

## A manipulated building agent

The agent that builds a target reads the system more than anyone. If it is steered, the thing it can do is write target files that do something other than they claim. Four barriers stand in the way.

1. **Exploration cannot write, and drafts can touch only test records.** Enforced in the engine's sandbox, as section 401 describes, with what "cannot write" means stated per archetype by each pack ([section 218](../2-kernel/218-discovery-and-findings.md)).
2. **A draft cannot widen a target's reach by itself.** Adding a surface or a destination outside the origins a person registered for the target needs that person's confirmation. A sign-in is held to the same rule: one that only the drafts declare sends no stored credential, and no token, until the person has been shown every address it sends to and has carried it out on its page; a credential belongs to the target it was stored for ([decision 0248](../../decisions/0248-a-sign-in-goes-only-where-a-published-version-says-and-a-credential-belongs-to-its-target.md)).
3. **Publishing needs a person, and the person is shown what matters first.** The compiler produces a review summary ordered by risk, above the ordinary file differences:

| Shown first | Why |
| --- | --- |
| New or changed surfaces and destinations | Where data can now go |
| New uploads and downloads | What can now leave or arrive |
| Effect classes that rose; new `irreversible` operations | What can now be changed |
| Changed identity strategies and read-back rules | Whether writes are still verified |
| Changed expressions in bodies and mappings | What values are now sent |
| Changed descriptions | What agents will now be told |
| What each tool starts and sets; every address a tool or a sign-in's credential can reach, by kind of place | What runs on the machine, and where data and credentials can go ([decision 0252](../../decisions/0252-a-fixed-place-a-review-that-shows-what-runs-and-a-sign-in-before-its-probe.md)) |

4. **The linter looks for the obvious.** Credentials in files, undeclared destinations, and descriptions that address the reader as an instruction. The rule for a description that addresses the reader lists its patterns in the lint's own documentation, with a test for each: an imperative sentence aimed at the model, a mention of a tool this version does not publish, a mention of a file or address outside the target, and a request to hide something from the person. It helps the reviewer and is known to be evadable.

## Targets written by someone else

A shared target pack or a bundle from another machine is unreviewed code.

- Its signature is checked, and an unsigned one is refused unless a person overrides for that file.
- Importing shows the same review summary as publishing.
- An imported version starts with its write operations disabled. They are enabled only after the local publish gates pass against the importer's own environment.
- Plug-ins it needs are not installed for it; they are requested, and an administrator decides.

## An agent cannot loosen its own limits

- Policy, grants, and toolsets are changed only by administrators, through audited actions.
- On one machine the person holds every role, so an agent with a shell could try to change a setting. **Protected settings** (approval rules, record limits, allowed file roots, capture level on writes, and the guardrail switches) can be changed only with the same operating-system user-presence prompt as an approval ([section 503](503-approval-and-sign-in-surfaces.md)).
- Limits on records per run, rate, and quota cap what any single call can do, whatever the agent intended.

## What this cannot do

Anything2MCP controls what passes through Anything2MCP. An agent that has been granted a read tool can read what that tool returns, and the agent's other tools are outside Anything2MCP's control. Anything2MCP is never an MCP client: a hostile server loaded beside it in the same agent can write text that steers the agent's use of this product's tools, and only the agent's host can stop that. The annotations the product sets are true of the tool, but a client has no way to know it, so nothing in the product relies on a client honoring them. Sensitivity labels decide what reaches an agent in the first place; after that, the organization's own controls on the agent apply.

## What to build and test

| Item | Phase |
| --- | --- |
| Typed template positions and encoders, with tests that try to break out of each | 4 |
| Allowed file roots and content-type checks | 3 |
| Server-side blob uploads | 6 |
| The risk-ordered review summary | 5 |
| Protected settings | 3 |
| Import with write operations disabled | 6 |
| A test suite of manipulation attempts, run against every release | 9, started in 3 |
