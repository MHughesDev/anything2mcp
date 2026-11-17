# 213. Effects, hazards, and consent

A universal runtime starts programs, opens files, reaches networks, and talks to devices. This section says how each of those is declared, who checks it, and when a person must agree. It extends [section 212](212-policy-and-approvals.md) and [section 502](../5-security/502-defending-against-a-manipulated-agent.md); nothing there is loosened. Decision: [record 0052](../../decisions/0052-effects-are-enforced-and-hazards-raise-the-approval.md).

REA declares ten effects for each of its tools and derives every hint from them, which is worth copying. It also says plainly that its declarations are hints and that the host must enforce ([research note 16](../../research/16-rea-methods.md)). Here the runtime is the host, so here they are enforced.

## Three questions, three answers

| Question | Answered by | Decided where |
| --- | --- | --- |
| What may this provider do on this machine? | Its declared **effects** | The manifest, approved when the provider is enabled |
| What may this operation change in the target? | Its **effect class**: `read`, `write`, `irreversible` | The target's files; derived by the compiler; unchanged |
| What could this operation damage beyond the record it names? | Its **hazard** | The target's files; shown first in the review summary |

## Effects

| Effect | Declared with | Checked by |
| --- | --- | --- |
| Starts a process | Which programs, by path and digest; and whether programs a published plan names may be started | The supervisor, scoped to what the manifest and the published plan declare: nothing else can be started ([section 204](204-supervising-subsystems.md)) |
| Reads files | Which roots | The kernel's file access, for packs: the checked path in `a2m-provider`, which only the check against the allowed roots of section 502 can make. A pack receives checked paths and never resolves one |
| Writes files | Which roots | The same |
| Uses the network | Which destinations | The surfaces a target declares; the kernel's clients |
| Holds a session | For which kind of surface | The session keeper |
| Runs code that came from the target | Where, and confined how | The supervisor. Off unless a person enables it for that target |
| Attaches to a running process | Which, and proven how | The supervisor: only what the person named |
| Changes what is on a person's screen | Focus, windows, input | The scheduler, which serializes such work |
| Changes a device | Which link, which device | The supervisor, with exclusive use of the link |

**How strongly each is held is stated, not implied.** For every effect a provider declares, the listing shows one of three words.

| Word | Meaning |
| --- | --- |
| `enforced` | The runtime performs the effect itself, so nothing outside the declaration can happen. True of first-party packs |
| `confined` | The provider performs it, inside a confinement the operating system applies |
| `declared` | The provider performs it, and nothing but its signature and review stands behind the declaration |

A provider whose effects are only `declared` is trusted code, and the person enabling it is told so in those words.

**Everything an agent is told about a tool is derived from this record.** The read-only, destructive, idempotent, and open-world hints of the MCP protocol are computed from the effect class, the hazard, and the effects. Nothing is derived from a tool's name.

## Hazards

The effect class says what kind of change an operation makes to the record it names. A hazard says what else could be harmed.

| Hazard | Meaning | Example |
| --- | --- | --- |
| `none` | Nothing beyond the named record | Updating a line item |
| `data_elsewhere` | Data beyond the named record could be lost | A command that clears a table; a save that rewrites a whole file and cannot show that the rest is unchanged |
| `sends_outside` | Something leaves the system and cannot be recalled | A mail, a payment, a message to a queue others read |
| `device_function` | A device could be left unable to work | A firmware update; a network setting on the link we are using |
| `physical` | Something in the world moves, heats, opens, or stops | A valve, a motor, a lock |

| Hazard | The least approval | Also |
| --- | --- | --- |
| `none` | As the effect class says | |
| `data_elsewhere` | A person, every run | The diff states what else is at risk, in words, above the records |
| `sends_outside` | A person, every run | No automatic retry, ever |
| `device_function` | A person, every run; no standing approval; no pre-approval by policy | A precondition check must pass first, and a recovery route must be declared |
| `physical` | A person, every run; no standing approval; no pre-approval; a second person where policy asks | A declared safe state, a stop that is always reachable, and a rate limit |

An operation with any hazard other than `none` is `irreversible` unless its pack can show otherwise. A pack that can show nothing else changed may declare `none`: in the file pack, a patch save that passes the untouched check carries hazard `none` ([section 308](../3-packs/308-the-file-pack.md)). A pipeline's hazard is the worst of its operations', as its effect class already is.

## Commands are not a shell

For the command archetype ([section 201](201-archetypes-and-modes.md)):

- A program is named by path, and its digest is recorded.
- Arguments are built from a typed template, position by position, exactly as request templates are ([section 502](../5-security/502-defending-against-a-manipulated-agent.md)). No input is ever joined into a line that a shell reads.
- The environment is an allowlist. The working directory is declared. Output is bounded.
- A tool that accepts free text and runs it is not a tool this design can verify, and is refused.

REA's device tools follow the same rule: every one takes a fixed argument list, and none accepts a command line.

## Before and after an effect at a low level

Four habits, each taken from how REA handles the few places where it does change something.

| Habit | Why |
| --- | --- |
| Take the bytes to be sent before sending, and record their digest | The record then names exactly what was sent, whatever changes on disk afterwards. Section 502 already does this for uploads |
| Probe the state first, and treat an unclear probe as a stop | Rule seven: unknown means stop |
| Report the read-back as its own fact: matched, mismatched, or not available | A write that could not be checked is never reported as checked |
| Set a safety setting, read it back, and go on only if it took | A setting assumed is a setting not set |

## Consent

Some things need a person's agreement before discovery, not only before a write.

| Act | Agreement |
| --- | --- |
| Capturing a person's use of a system | That person starts and stops the capture; they are told what is kept |
| Installing a bridge into a host application | The person installs it; it is listed and can be removed in one action |
| Attaching to a process the runtime did not start | The person names the process; on one machine it is a protected setting |
| Running code that came from a target | Enabled for one target, by a person |
| Opening a link to a device | The person names the device |
| Enabling a provider whose effects are only `declared` | The person is told what that means |

Consent is stored as a record: who, when, for what, and in the person's own words where they gave any. It is never recorded on a person's behalf, and never inferred.

## Rule five, restated for inspection

Being able to look inside a system does not make every use of what is seen acceptable.

- Nothing learned by inspection is used to get past a sign-in, a second factor, a bot defense, a licence or copy check, a signature check, or a protection against reading a device.
- Reaching a debugging or administration endpoint does not show that one is permitted to use it.
- Inspecting a system one does not own may be limited by its terms or by law. The authorization record required before publishing on a server applies to what was discovered as much as to what is run.

REA states the same limits for itself.

## What is treated as untrusted

Everything that originates in a target: text in records, names in a listing, descriptions a site declares for agents, output of a program, strings in a file. It is marked as such in every result, limited in length, and never placed where it could read as an instruction. The marking is on by default, and a tool is exempt only by a reviewed list.

## Open points

- Which operating-system confinement backs `confined` on each platform. Until researched, only first-party packs and nothing else can claim more than `declared`.
- Whether hazards should be a closed list or allow a target to add its own. Closed, for now.
- The exact preconditions a `device_function` operation must declare; [section 310](../3-packs/310-the-device-pack.md) starts the list.
