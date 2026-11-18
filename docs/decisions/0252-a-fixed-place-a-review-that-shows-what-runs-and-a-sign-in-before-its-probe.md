# 0252. A record that is one fixed place, a review summary that shows what a tool runs, and a sign-in declared before its probe

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** [section 205](../spec/2-kernel/205-target-files.md), [section 209](../spec/2-kernel/209-record-identity-and-dependent-records.md), [section 403](../spec/4-building-tools/403-testing-and-publish-gates.md), and the design of [`a2m-files`](../architecture/crates/a2m-files.md). It closes the gap [decision 0246](0246-the-file-pack-reads-folders-settings-files-and-growing-files.md) named for hypothesis H-26, gives the review summary the place [decision 0216](0216-the-command-pack-on-three-systems-the-administrators-group-and-a-script-the-target-keeps.md) asked for, and loosens the one check the sign-in package of [decision 0205](0205-a-sign-in-an-agent-asks-for-is-carried-out-by-its-person-on-the-engines-page.md) ran into. It gives fields to the rule of [decision 0248](0248-a-sign-in-goes-only-where-a-published-version-says-and-a-credential-belongs-to-its-target.md), which another package is writing.

## Context

Three packages met the compiler today where it asked for something a target could not honestly give. A write of one fixed place had to name its record by an input its binding never used. The publish review showed a tool's effect, hazard, surfaces, and declared destinations, and nothing of what it starts, what it sets in a program's environment, or where a credential goes. And a sign-in could not be declared until a read compiled, though for a system that answers no read before a sign-in, nothing can be drafted first.

## Decision

### 1. A record that is one fixed place

An operation declares `identity: {strategy: natural, place: fixed}` and no identity input. It compiles only where:

- every binding's lowering names the place it reads and writes, through the new `Lowering::fixed_place`, which gives a `FixedPlace` (a file kind and a name) and by default gives none;
- every binding names the same place, on the same surface;
- the file kind that holds the place is the lowering's own pack's, or one it declared in `reads`, for the binding's surface kind; and
- that file kind's new `FileKind::fixed_places`, asked of the target's own documents on the binding's surface, lists the name. Its default lists none.

The declaration is the file kind's, read from the document a person wrote, and the kernel holds the lowering to it: a lowering cannot make a place fixed that its file kind does not declare fixed in that document, and a kind whose places an input chooses (an entry of a folder by its name) declares none. Anything else is `kernel.place_not_fixed`, a new code. The plan holds such an operation as `natural` with an empty key, which the compiler made in no other case; the review summary names the place (`ReviewItem::place`).

What each ledger rule that needs a record's key means for it (section 208):

| Rule | For a record that is one fixed place |
| --- | --- |
| Identity before action (the first principle) and rule 1, intent before action | The intent is stored under the record's key before the save. The key is the engine's name for a record with no key input, `place_key`: the operation's name and the record's place in the run. All of it comes from the target's files; nothing an agent supplies is in it |
| Keyed upserts (the third principle) | Two writes of the same values to the one place leave one result. A duplicate is impossible by construction: there is one place, and a create cannot make a second |
| Rule 2, proof before verified | The binding's read-back reads the place and compares, as for any `natural` record |
| Rule 5, reconcile after any doubt | Resume reads the place by the read-back alone. The place always exists or the read says it does not, so a read is proof both ways, unlike a record with no key ([decision 0185](0185-a-record-with-no-key-is-never-proved-absent-by-one-look.md)) |
| Rule 7, one lock per record | The lock is on the key above. Every run of the one operation takes the same lock. Two different operations that write one place take different locks: the place's own name is not in the plan yet (see "Not built") |
| The idempotency token | Made from the run and the key above, as for every record |

A step is handed no key for such a record, since its key inputs are none and its key in the run names nothing in the system (section 209, "A key is never guessed").

### 2. The review summary shows what a tool runs, sets, and reaches

The summary moves to its own module of `a2m-files`, `review`, and gains typed fields, filled from what the compiler and the packs already know:

- `ReviewItem::bindings`: for each binding, the operation, mode, surface, the programs the surface declares (`SurfacePlan::programs`, the kernel's own reading), and `states`. `BindingStates::Stated` lists `runs` and `environment`; `BindingStates::NotStated` carries the fixed sentence `NOT_STATED`, "this kind of tool does not state what it runs". A list that is empty under `Stated` means none.
- `RunReview`: a `ProgramRun` (a program's path and digests; or a fixed pair: the interpreter's path and digests, its fixed arguments, the kept script's path and digest) and, for a script, the fixed `SCRIPT_NOTICE`: "runs a script this target keeps: the digest pins its bytes; it does not make it safe".
- `EnvironmentVariable`: every variable a binding sets or passes, by name, with `VariableValue::Set(value)`, `Secret` (no value, ever), or `Passed`. The pack refuses the closed list of variables that redirect what a program loads; the review shows the rest.
- `ReviewItem::reach`: every address the tool can send a request to, from its permissions' destinations and from what each binding's pack states (`Sends::Only { surface, addresses }`; `surface` adds the surface's address in every environment), grouped by `AddressScope` ([decision 0240](0240-one-typed-reading-of-where-an-address-is-and-one-question-of-a-plain-link.md)). `Reach::Stated` says whether any address may leave this machine (`ScopedHost::may_leave_this_machine`); `Reach::Partly` carries the known addresses and the fixed `REACH_NOT_STATED` wherever a binding's pack does not say, or says that what it starts may reach anywhere (`Sends::NotStated`, the default, and the honest answer for a program).
- `ReviewSummary::sign_ins`: one `SignInReview` per entry of `auth.yaml`: its place in the plan's `auth`, the surfaces, provider, sign-in mode, each credential by role and reference (never a value), the probe (`ProbeReview::Declared` or `Pending`), and for each environment `SentTo`. The compiler cannot know where a provider sends, so it fills `SentTo::NotStated`; `ReviewSummary::state_sent_to(entry, environment, Some(addresses) | None)` replaces it with the addresses grouped by kind of place, or `ProviderNotHeld`.

A pack states this through one new method, `Lowering::statement(binding, lowered, cx) -> Statement`, asked after `lower` with the same context. The default is `Statement::NotStated`, so every pack builds and behaves as before, and its tools say plainly that they do not state what they run.

**Order.** Tools stay ordered by hazard, then effect class. Between those and the name, a new key: first where the tool sends (a reach not fully stated, then one that leaves the machine, then one that stays), then what it starts (a binding that does not say or that starts a kept script, then a program, then nothing). Hazard and effect say what a tool can damage, and stay first; among tools that can damage the same, the one that can expose data to another machine, or run code the target keeps, is the one a person should read first. An unknown sorts as the worst case, because unknown means stop (rule 7). Inside a reach, kinds of place go from an address that cannot be read, through public, a name (which may lead anywhere, `localhost` included, [decision 0188](0188-credentials-over-plain-links-drafted-workbooks-and-published-device-maps.md)), and the nearer ranges, to this machine last. Sign-ins are their own list: one whose probe is not drafted first, then in the file's order; a page shows them before the tools, since a credential sent to an address is a risk to every tool of its surfaces.

### 3. A sign-in may name its probe before the read is drafted, in a draft only

In a draft compile, a probe that names nothing, where no operation file of that name exists either, compiles to the new `ProbePlan::Pending { name }`, with the warning `kernel.probe_pending` in the lint result and `ProbeReview::Pending` with the fixed `PROBE_PENDING` in the review. A probe that names an operation with errors stays that operation's errors. A publish compile refuses a probe nothing defines, as `kernel.unknown_name`, as before; and `Plan::check` in `a2m-core` refuses a `Pending` probe in a plan under any version but the draft version, `0.0.0`, so no published plan can hold one, whoever made it. A session made under a pending probe is never called confirmed: the keeper's probe has nothing to run.

## Alternatives weighed

- **A sixth identity strategy, `fixed`, in `a2m-core`.** Truest to the words, and the record's place name could be the strategy's key. Not taken: `IdentityStrategy` is matched without a wildcard in the engine and in nine packs' `Lowering::identity`, so the variant breaks the build of eleven crates, several of which other agents have open tonight, and every pack would then answer twice for one fact (`identity` and `fixed_place`). `natural` already means "found by the system's own key"; for a fixed place that key is the place, and `place: fixed` says where it comes from. The variant remains open if the plan gains the place's name.
- **The place's name in the plan (`IdentityPlan::place`).** It would put the place itself in the ledger's key, so two operations that write one place share a lock. Not taken now: every plan struct is built by struct literal in the engine's tests, so any new field breaks a crate another agent is editing. It is the next step, with the lines for the engine given below.
- **Each pack renders its own review lines.** A pack would hand text. Refused: a page would then print sentences built from a target's or a system's words, and nothing would make a silent pack visible. The fields are typed, and the only sentences are the kernel's constants.
- **Loosening the probe check in publishes too, with a warning.** Refused: a session nobody can check is a session nobody can trust, and a published version is what policy trusts.

## What each other crate must adopt

- **`a2m-pack-file`:** `fixed_places` on the `documents/` kind (each record that declares no `key`, named `document.record`) and `fixed_place` on the lowering (a binding with no `key:` names `document.record`); its four fixtures that name a record by an unused input (`plist_set`, `plist_rate_set`, `ini_note_stripped_set`, `ini_orders_set`) declare `identity: {strategy: natural, place: fixed}` and drop the `settings` input, and `tests/entries.rs` stops passing it. Then `statement`: its bindings send to no address unless a surface's path is on a network share.
- **`a2m-pack-command`:** `statement`, from its section: the program (the surface's address and its digests), or the fixed pair (interpreter, fixed arguments, script and digest); `env_set` as `Set`, `env_pass` as `Passed`; `Sends::NotStated`, since what a program reaches is the program's.
- **`a2m-pack-http`, `a2m-pack-mcp`, `a2m-pack-web-ui`:** `statement` with `Sends::Only`: the surface's address, and any address an endpoint or a tool names beyond it.
- **`a2m-actions`:** render `ReviewSummary` on the publish page from its fields, sign-ins first, instead of the text lines it builds from `ReviewItem`; call `state_sent_to` for each entry and environment with what the keeper's `sign_in_addresses` returns (`None` for `AUTH_PROVIDER_UNKNOWN`); treat a sign-in whose probe is `ProbePlan::Pending` as a draft sign-in that goes to the person, and never call the session it makes confirmed.
- **`a2m-auth`:** nothing new: `sign_in_addresses` already gives each address and what is sent there. A keeper that runs probes must answer `Unknown` for a `Pending` probe.
- **`a2m-engine`, when the plan carries the place:** key such a record by the place's name.

Other packs have the same case and are not changed here: a device's single register or setting at a fixed address in its map; a desktop or web settings screen that shows one record and whose reach names no input; an HTTP singleton resource such as `PUT /settings`; a retained message on a fixed topic of a stream.

## Not built

- The place's own name in the plan and in the ledger's key, so that two operations that write one place share one lock.
- No pack states anything yet: every tool's summary says "this kind of tool does not state what it runs" until each pack adopts `statement`.
- The page that renders the new fields is `a2m-actions`'.

## Consequences

- No plan's bytes change: the example target compiles to the plan it compiled to before, and the plan's pinned hash is unchanged. `ProbePlan` has a new variant that only a draft can hold. The operation file's `identity` takes an optional `place`, so the published schema of operation files changes.
- `ReviewItem` and `ReviewSummary` keep their paths at the crate root and gain fields; `ReviewSummary` gains `sign_ins`.
- Two kernel lint codes: `kernel.place_not_fixed` and `kernel.probe_pending`.
