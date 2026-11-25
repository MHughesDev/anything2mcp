# 218. Discovery and findings

Discovery is how the runtime learns what a system can do. This section defines what a discoverer may do, what it returns, and the rules that stop a guess from passing as a fact. Much of the discipline here was learned from the REA project ([research note 16](../../research/16-rea-methods.md)); the design is ours. Built today: partly; [section 704](../7-project/704-readiness.md) gives the detail.

## Three rules first

1. **Discovery cannot write.** Discovery runs as a job: a runner calls the discoverer inside the engine's sandbox, which gives it only what the sandbox's port offers, and writing is not among it. What "cannot write" means differs by kind of system, so each pack states it for its access kind and its conformance test proves it: for the web pack, a request that is not a read is never sent; for a program, it is started only in the help and version forms the target declares; for a database, every read runs in a transaction the store holds read-only; for a queue, nothing is consumed. A discoverer that needs a system to change in order to observe it is not a discoverer.
2. **Discovery returns findings, and a finding cannot run.** Only a compiled target runs. A finding becomes something that can run only as target files that pass the compiler and the gates, whoever or whatever wrote them: a person, an agent, or the drafter of [section 402](../4-building-tools/402-the-scout-and-who-does-what.md).
3. **A finding claims the least it can.** Where a field is not given, it takes the weakest value, never the strongest.

**Capturing a person's own use is not exploring.** When a discoverer explores, a write is impossible, by rule 1. When it captures a person using the system in a tab or window they drive, such as the web pack's watch tab, the person may save, because the writes are theirs: the capture only records what passes, redacted, and sends nothing of its own. The findings of a capture are `discovered` for what was seen and say nothing of what was not used; a path given a placeholder from several requests, and every draft made from a capture, is `inferred`. A capture is a discoverer that a person drives: an agent may ask for one, only its person begins it, on the engine's own page past the presence prompt, and the scout never starts one ([decision 0392](../../decisions/0392-a-page-that-reads-by-a-method-that-may-write-is-learned-from-a-persons-watched-use-and-read-through-an-asserted-request.md); designed, not built).

## A finding

| Field | Meaning |
| --- | --- |
| `id` | A digest of the finding's content: subject, kind, what was found, source, level, coverage, limitations. Display names and local paths are left out, so the same observation has the same identifier everywhere |
| `subject` | What the finding is about, as one tagged identity (below) |
| `kind` | A registered name, such as a surface, a screen, a field, an action, an endpoint, a command, a function, a schema, a kind of record, a sign-in method, an event, a register. Each kind has one owning pack, which registers its shape; another pack may read findings of the kind as data ([decision 0062](../../decisions/0062-words-that-arrive-with-packs-are-registered-names-and-packs-share-data-never-code.md)) |
| `found` | The thing itself, in the form its pack defines |
| `level` | `discovered` or `inferred`. Nothing higher; the rest of the ladder belongs to drafts ([section 219](219-the-evidence-ladder.md)) |
| `source` | Who vouches for it (below) |
| `coverage` | `complete`, `partial`, or `unknown`: how much of what could have been looked at was looked at |
| `limitations` | Coded reasons the finding is weaker than it looks, each with a sentence |
| `derived_from` | The findings this one was worked out from |
| `provider` | Which discoverer produced it: name, version, and the digest of any outside component it used |
| `observed_at`, `evidence` | When, and references to the stored observation |

## The subject

Every finding names its subject. "The target" is not enough: a rebuilt program at the same path, or a different page at the same address, must not look like the thing that was observed.

| Tag | Identified by |
| --- | --- |
| `file` | Its digest, its format, and which part of it where a file holds several |
| `address` | An origin and path, with no credentials in it |
| `process` | Its identifier, its start time, and the digest of its program |
| `session` | One of the runtime's own sessions, and what that session was attached to |
| `device` | The identifier the device reports, with its model and the firmware version it reports |
| `unknown` | A reason from a closed list: derived from other findings, not observed, not supported, withheld |

Where the system reports its own version, the subject carries it. A change of version is what makes levels fall.

## Level and source are two things

The level says whether the thing was observed. The source says who is vouching. REA keeps these apart, and so do we.

| Level | Rule |
| --- | --- |
| `discovered` | The thing was observed on the live system: an answer received, an element present on a screen, a row returned, an entry in a listing the system computed about itself |
| `inferred` | Everything else: a description, code, documentation, a pattern, reasoning |

| Source | Meaning | Highest level it can support |
| --- | --- | --- |
| `live_system` | The running system, observed | `discovered` |
| `published_description` | A description the system's owner publishes: a schema, a contract, a manual | `inferred` |
| `artifact` | The system's files or code, read without running it | `inferred` |
| `third_party` | Someone else's account of the system | `inferred` |
| `reasoning` | A person's or an agent's conclusion | `inferred` |

A description that says an endpoint exists is a claim. The endpoint answering is an observation. Both are useful, and they are never shown as the same.

**A derived finding is never stronger than its weakest parent.** One guessed segment in a path makes the whole endpoint `inferred`.

## Coverage is not content

An empty result means nothing by itself.

| Coverage | An empty result means |
| --- | --- |
| `complete` | There is none |
| `partial` | None was seen in the part that was looked at |
| `unknown` | Nothing can be concluded |

"No endpoint was seen for deleting an order" after a five-minute capture is `partial`, and says so. Missing coverage is unknown, never absence.

## Limitations

Each limitation is a code from a closed list and a sentence. The list below and the reasons for an unknown subject are fixed in `a2m-core` as `LimitationCode` and `UnknownReason`; kinds of finding are registered names, which each discoverer brings. Codes, so that a machine can act on them; a sentence, so that a person understands.

| Code | Meaning |
| --- | --- |
| `truncated` | The observation was cut at a size or count limit |
| `time_limit` | It was cut at a time limit |
| `sampled` | A sample was looked at, not the whole |
| `not_exercised` | The thing was seen to exist and never seen to work |
| `redacted` | Something was removed before storing, such as a secret |
| `permission_denied` | The identity used was not allowed to see more |
| `unverified_component` | An outside component did not match a recorded digest |
| `single_variant` | One variant of a screen or response was seen; others may exist |

## Comparing what is described with what is observed

Two discoverers often look at the same thing from two sides: one reads a description or the code, the other watches the system in use. Comparing them is a discoverer of its own, and the idea is taken directly from REA.

| Result | Meaning | What it must not be read as |
| --- | --- | --- |
| `matched` | Described and observed, exactly or through a path template | Proof the description is right in every detail |
| `described_only` | Described, and not seen in this observation | Unused, hidden, or broken. Only that this observation did not exercise it |
| `observed_only` | Seen, and not in any description | Undocumented by intent. Descriptions are often incomplete |
| `unknown` | The comparison could not be made, with a reason | Either of the above |

`described_only` is reported only when the observation covered the same origin and its coverage is known.

## The unknowns ledger

What is not known is recorded as carefully as what is. Each target has a ledger of open questions.

| Field | Meaning |
| --- | --- |
| The question and its scope | "Does saving a line item ever show a confirmation dialog?" on `order.line_item_form` |
| Why it matters | Which draft or tool depends on the answer |
| What would settle it | The kind of evidence needed, and at what level |
| Status | Open, being looked at, blocked, settled, or out of scope |
| Settlement | The findings or gate evidence that settled it. A question is settled by evidence, never by a statement |

An unknown that a tool depends on is shown in the review summary before publishing. A person may publish with it open; they do so knowingly, and the tool's record says so. This is the paper form of "unknown means stop": at run time an unseen variant is a stop, and at build time it is a line in the ledger.

## What a discoverer reads, by access kind

| Access kind | Observed on the live system | Read as description or capture |
| --- | --- | --- |
| Interface | The accessibility tree as a snapshot; which screen models match | Nothing by default |
| Request | Captured exchanges while a person uses the system; a listing the service computes, such as an MCP server's tools | A published schema or contract; the system's own code, through an optional provider |
| Command | The program's own help and version output, in the forms the target declares | Its manual; completion data; a PowerShell module's help file and manifest |
| Bridge | The functions the host lists | A type library, a dictionary, headers |
| File | The structure of a real file: sheets, named ranges, keys | A format's published schema |
| Stream | Topics the broker lists; a bounded sample of messages | Registered schemas |
| Device | What the device answers when asked to identify and list itself | A published map of registers or objects; a firmware image, through an optional provider |

A description or an artifact is read from the system, or from a file a person hands the discovery. A discoverer declares in its manifest the names under which it reads such a file (`files`). A call names each file of the project's `inputs` folder under one of those names; the registry checks the path and the name when asked, and the engine checks the path again under a read-only root of that folder before any discoverer runs, so a discoverer is handed that file and no other. A name no discoverer of the surface reads is refused, never ignored. The files are part of the discovery's stored call ([decision 0263](../../decisions/0263-a-person-hands-the-scout-files-of-the-projects-sources-folder-by-names-the-discoverers-declare.md)).

## Bounds

- Every discovery has a time limit, a size limit, and a depth limit, and reports which it hit.
- Capture is redacted before anything is stored, by the same redactor as evidence ([section 217](217-traces-evidence-and-repair.md)).
- A discovery that samples data from a system stores shapes, not values, unless a person asks for values and policy allows it.

## Stored findings do not migrate

Findings are stored with a format number. When the format changes, old findings are not rewritten into the new one, because rewriting would claim an observation that was never made in that form. They are kept readable, marked as old, and re-observed.

## Open points

- Whether a help listing counts as the live system observing itself or as a description. It is the running program's answer, so `discovered` for "this command is listed", and `not_exercised` until it is run.
- How long a finding stays valid; see section 219.
