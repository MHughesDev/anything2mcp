# 0330. Every name says what the part is, in plain words

**Status:** Accepted. The rule in point 1 is an owner decision. The names in point 2 are design decisions under it, made by the lead under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md); the owner had not read the list when it was merged. Adds to [decision 0301](0301-every-part-has-a-proper-name.md).

**Date:** 2026-11-23

**Changes:** the glossary, every document, and the code's names of the parts in point 2. Roadmap package N0 carries the renames, with the one of [decision 0329](0329-the-compiled-file-the-engine-runs-is-the-compiled-target.md).

## Context

The owner found "plan" confusing because its everyday meaning is not what the part is (decision 0329), and asked that every settled name be checked the same way. A review of about 470 names found about twenty-five that mislead a plain reader: a word whose everyday meaning differs, a word that already names another part here, or jargon where two plain words say it.

## Decision

1. **The rule.** A name says what the part is, in plain words a reader outside the project would read the same way. A name is changed when its everyday meaning differs from the part, when it already names another part here, or when it is jargon that plain words can replace. Standard terms of the field that mean what they say here are kept.
2. **The renames.** Each old name becomes the new one everywhere it names this part.

   | Old name | New name | What it is |
   | --- | --- | --- |
   | Telemetry, and its crate | Logs and Traces, and the crate renamed to match (logs) | The crate that writes logs and traces on this machine, secrets removed; nothing is sent anywhere |
   | Field test, Field harness, Field prompt | Copy test, Copy Test Harness, Copy Test Prompt | A run against a copy of existing software the project starts itself; its harness; its presence prompt (`field_harness` becomes `copy_test_harness`) |
   | Identity strategy | Record matching | How a written record is found again |
   | Archetype | Access kind | One of the seven ways a system is reached |
   | Artifact (the archetype) | File | The access kind that reads and writes files |
   | Artifact (the evidence) | Capture | A snapshot, a network exchange, or a screenshot kept as evidence |
   | Owner (of a session) | Session owner; Owner Mark becomes Process Mark | The person a user session belongs to. "The owner" alone names the project's owner |
   | The project's `sources` folder, Handed Files | `inputs/`, Input Files | The files a person hands the scout. A target's `sources/` keeps its name |
   | Decision point | Run question | A question an operation puts to a person during a run |
   | Control (what the engine asks of the control plane) | Control Port | A control remains a widget on a screen |
   | Probe (of a session) | Session Check | The cheap signed-in read. Probe keeps the sense of a target's first inspection |
   | Held folder | Folder Handle | A folder kept open by handle so its path cannot be swapped |
   | Managed component | Required Program | An outside program a provider needs, never bundled or fetched |
   | Generation (of a key) | Key Rotation Number | Which version of a key |
   | Part (of the specification) | Chapter | One of the seven divisions of the specification |
   | Window (finding kind) | Window Shape | The shape of one window of a running application |
   | Profile | Deployment Shape, Browser Profile, Client Profile | Three different things the one word named |
   | Identity mode, Sign-in mode | Identity Kind, Sign-In Kind | "Mode" names only `ui`, `http`, `mcp` and the other modes |
   | The command bridge | The command stdio | The command that starts the Stdio Proxy |
   | Recorded Catalog | Action Catalog | The digested list of every action |
   | Application's verdict | System's verdict | The system's own answer to a sent write |
   | Network Standing | Place Reading | What joins interfaces and routes into a place per address |
   | Approval Desk, Renewal Ladder, Terminal Finality, Lowering, Emission | Approval Recorder, Renewal Order, Final State Rule, Step Expansion, Output Writing | Names on the architecture page |
   | Source (the extension point) | Source Reader | One name for one part |
   | Scout Pipeline | Scout Run | The scout is a fixed procedure, not a pipeline |
   | Check (of a watch) | Watch Check | One of several checks |
   | Hold (of the scheduler) | Scheduler Pause | Beside the Holding Folder, no other "hold" name is left |
   | Fixture record (the action's name in prose) | Fixture Recording | The action id `fixture.record` stays |

3. **Kept,** because each means what it says here: MCP tool, session, lease, envelope, blob, principal, commit point, binding, ledger, engine, kernel, pack, provider, mode, scout, drafter, shadow, candidate, kill switch, guardrail, preset, diff, approval, snapshot, surface, watch, baseline, gap, coverage, quarantine, holding folder. The Version row's "snapshot of a target" becomes "published state".
4. **Old records and journal entries are not rewritten.** They keep the names of their day; the glossary lists each old name beside its new one.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Rename only "plan" | The same fault is in about twenty-five other names |
| Rename in the documents only | The code would then use one vocabulary and the documents another, against the code conventions |
