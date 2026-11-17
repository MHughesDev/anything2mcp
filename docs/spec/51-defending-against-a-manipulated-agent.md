# 51. Defending against a manipulated agent

Agents read text they did not write: records, notes, pages, files. Any of it can contain instructions aimed at the agent. Anything2MCP therefore assumes that the agent calling a tool, and the agent building one, may at some moment be acting for someone else. This section collects what the design does about that, and adds the rules that were missing.

The aim is simple to state: a manipulated agent should be able to do nothing through Anything2MCP that the person could not have been asked to approve, and should not be able to make Anything2MCP reach, read, or send anything its tools do not openly declare.

## Inputs cannot change where a request goes

Operation inputs are filled into request templates. A crafted input must not be able to redirect the request.

- **Every placeholder has a position, and is encoded for it.** A path segment, a query value, a header value, and a value in a body are each encoded by their own rule. The compiler parses every template and assigns the positions; nothing is assembled by joining strings.
- **A path placeholder cannot contain a separator or a dot segment.** An input of `../admin` is a validation error, not a different path.
- **Scheme, host, and port never come from an input.** They come from the surface.
- **Redirects are followed only within declared surfaces.** Anything else stops with `policy` and the code `surface_not_declared`.
- **Inputs that are themselves addresses** use a `url` field type that accepts only destinations the operation declared. On a server, private and loopback addresses are refused unless declared.

## Inputs cannot make Anything2MCP read arbitrary files

Sources and uploads take files. A manipulated agent must not be able to name any file on the machine and have it sent to a system.

| Where | Rule |
| --- | --- |
| On a server | A caller never passes a path. It uploads content, which becomes a blob, and passes the blob's reference |
| On one machine | A file is read only from allowed roots: the project folder and folders the person has added. Links are resolved before the check. A pack never resolves a path: it receives a checked path, which only this check, in `a2m-provider`, can make |
| Everywhere | Size limits; the content must be the type it claims, so a renamed file is refused; a file is read once and stored as a blob, so what was previewed is what is uploaded |

## Downloads are data

- A downloaded file goes to a scoped store and comes back as a resource link, never inline.
- Its name is sanitized, its size is limited, and nothing opens or runs it.

## Nothing from the system becomes an instruction

- Tool names and descriptions are written by the target's author and reviewed by a person. Text captured from the system never flows into them unreviewed.
- Results are structured. Free text from the system sits only in fields marked untrusted, limited in length and stripped of markup.
- A validation message from the system, returned with a `rejected` result, is such a field.
- Snapshots and captures given to a building agent mark the system's text the same way.

## A manipulated building agent

The agent that builds a target reads the system more than anyone. If it is steered, the thing it can do is write target files that do something other than they claim. Four barriers stand in the way.

1. **Exploration cannot write, and drafts can touch only test records.** Enforced in the engine's sandbox, as section 26 describes, with what "cannot write" means stated per archetype by each pack ([section 58](58-discovery-and-findings.md)).
2. **A draft cannot widen a target's reach by itself.** Adding a surface or a destination outside the origins a person registered for the target needs that person's confirmation.
3. **Publishing needs a person, and the person is shown what matters first.** The compiler produces a review summary ordered by risk, above the ordinary file differences:

| Shown first | Why |
| --- | --- |
| New or changed surfaces and destinations | Where data can now go |
| New uploads and downloads | What can now leave or arrive |
| Effect classes that rose; new `irreversible` operations | What can now be changed |
| Changed identity strategies and read-back rules | Whether writes are still verified |
| Changed expressions in bodies and mappings | What values are now sent |
| Changed descriptions | What agents will now be told |

4. **The linter looks for the obvious.** Credentials in files, undeclared destinations, and descriptions that address the reader as an instruction.

## Targets written by someone else

A shared target pack or a bundle from another machine is unreviewed code.

- Its signature is checked, and an unsigned one is refused unless a person overrides for that file.
- Importing shows the same review summary as publishing.
- An imported version starts with its write operations disabled. They are enabled only after the local publish gates pass against the importer's own environment.
- Plug-ins it needs are not installed for it; they are requested, and an administrator decides.

## An agent cannot loosen its own limits

- Policy, grants, and toolsets are changed only by administrators, through audited actions.
- On one machine the person holds every role, so an agent with a shell could try to change a setting. **Protected settings** (approval rules, record limits, allowed file roots, capture level on writes, and the guardrail switches) can be changed only with the same operating-system user-presence prompt as an approval ([section 49](49-approval-and-sign-in-surfaces.md)).
- Limits on records per run, rate, and quota cap what any single call can do, whatever the agent intended.

## What this cannot do

Anything2MCP controls what passes through Anything2MCP. An agent that has been granted a read tool can read what that tool returns, and the agent's other tools are outside Anything2MCP's control. Sensitivity labels decide what reaches an agent in the first place; after that, the organization's own controls on the agent apply.

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
