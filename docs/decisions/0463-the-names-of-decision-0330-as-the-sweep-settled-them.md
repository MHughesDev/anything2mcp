# 0463. The names of decision 0330, as the sweep settled them

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md), in package N0b. The owner had not read this record when it was merged. The number 0463 is a placeholder the lead renumbers on merge.

**Date:** 2026-11-25

**Changes:** the code and the documents in force, for the names of [decision 0330](0330-every-name-says-what-the-part-is-in-plain-words.md), under the rule of [decision 0402](0402-the-rename-sweep-renames-every-name-and-keeps-what-is-stored.md) that what is stored or read from a file keeps its spelling; the glossary, which gives each old name beside its new one; the documentation audit, which lets history keep two old crate names and the old command.

## Context

Decision 0330 names each part's new name and decision 0402 says what keeps the old one. Building the sweep met choices neither settles: a name 0330 gives that already names another part, a stored word 0330 itself renames, names 0330 gives for prose that the code must spell, and one name whose scope the table leaves open.

## Decision

1. **Stored and read words stay, even where 0330 names a new one.** 0330 says `field_harness` becomes `copy_test_harness`; it is the word stored in `approvals.presence_check`, so it stays, under 0402's rule, and the Rust variant is `CopyTestHarness`. The same holds for: the manifest's key `archetype` and the access kind File written `artifact`; a trace's field `artifacts`; the compiled target's `decision_points` and an operation's key of that name; an identity's key `strategy`; the ledger's `generation` columns and the bound labels `generation` and `owner_key_generation`; the variable `ANYTHING2MCP_OWNER_MARK`; the cause `revoked_by_owner`; the settings keys `telemetry.export`, `telemetry.crash_reports`, `profile.name`, `profile.preset`, and `sessions.probe_minutes`; the `auth.yaml` keys `probe`, `probe_inputs`, and `mode`; the http step kind `probe`; the session events `probe_passed` and `probe_failed`; a watch's stored reason `on_hold`; the table `scheduler_holds`; the folder `profiles`; and the key `identity_mode` of an action's inputs, a grant, and a policy rule, which a policy file also carries. Each is named where it is defined.
2. **The code's names for the parts.** The logs crate is `a2m-logs` and its handle `LogsAndTraces`, with `LogSettings`; the trace's capture types are `CaptureKind`, `OfferedCapture`, `CaptureOutcome`, `CaptureRef`, and `RedactedCapture`, since `KeptCapture` already names the engine's own; the access kind is `AccessKind`, the session check's types `CompiledSessionCheck`, `SessionCheckOutcome`, and `RunSessionCheck`, the key rotation number `KeyRotationNumber`, the control port `ControlPort`, the scheduler pause `PauseScope` with `Scheduler::pause`, and the profiles `DeploymentShape`, `ClientProfile`, `BrowserProfileKey`, `BrowserProfileRef`, and `PlatformProfile`.
3. **Public names renamed with no alias.** No release has been kept, so `contracts/next-breaks` lists nothing (a listed break that does not happen fails the check) and no alias is added: the failure codes `telemetry_settings_invalid` and `on_hold` are `log_settings_invalid` and `scheduler_paused`; the lint codes `kernel.probe_invalid` and `kernel.probe_pending` are `kernel.session_check_invalid` and `kernel.session_check_pending`; the command `bridge` is `stdio`; the project folder `sources/` is `inputs/`; the scout's output field `archetype` is `access_kind`. An older project's `sources/` folder and a client file that names `bridge` must be changed by hand.
4. **A name that already named another part.** The glossary's Capture named a recording of traffic for discovery; 0330 gives the name to the evidence, which the code already called a capture. The recording is a **traffic capture**.
5. **The architecture page's names stay on the page.** Lowering, Emission, and the other names of 0330's page row are renamed on the page and in prose. The provider contract's trait `Lowering` keeps its name in code: it is a public contract, and "lowering" is the field's word for what it does.
6. **Words that name something else keep their name:** a finding's source `artifact`; a build's or a release's artifacts; a page generation and a scheduler's wait generation; a target's first inspection (`target.probe`) and a tab's hang probe; a part of a large target; the project's owner; the SDK's bridge to the local channel; a lock's holds.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Store `copy_test_harness` from now on and read both words | An alias needs a record that says to add one, and a reader of two words is a second rule for one value |
| Rename `identity_mode` in the actions' inputs too | The same key is read from a policy file a person writes, so it would stay there and differ from the input of the same name |
| Rename the trait `Lowering` | A change to a public contract for a page's name, which 0330 lists as a page name |

## What would reopen it

A release, after which every rename in point 3 is a break to list; or a reader confused by a kept stored word, which would argue for a migration and a new format number.
