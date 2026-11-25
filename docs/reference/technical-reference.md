# Technical reference

This file holds the engineering detail behind the [system design](../spec/README.md): interfaces, data model, protocols, and catalogs. A part that a package has built was settled by that package's decision record, and where this page and the code differ, the code is right. The rest is a starting design, refined by the package that builds it.

## Crate layering

Crates depend only downward, which keeps build times short and the core small. A crate may depend only on the layers its row names, never on a crate in its own layer. `tools/docs_audit.py` reads this table, compares it with the crates on disk, and refuses any other edge. The reasoning behind each layer, and which crate defines each interface, are in [the architecture](../architecture/README.md) ([decision 0061](../decisions/0061-the-layers-are-redrawn-so-that-every-needed-dependency-is-allowed.md)).

Layers 1 to 8 are the kernel. Layers 9 and 10 are the packs ([spec section 202](../spec/2-kernel/202-kernel-and-packs.md)).

| Layer | Crates | May depend on |
| --- | --- | --- |
| 1. Types | `a2m-core` | Nothing internal |
| 2. Foundations | `a2m-expr`, `a2m-vault`, `a2m-logs`, `a2m-netstate` | Layer 1 |
| 3. Stores | `a2m-files`, `a2m-ledger` | Layers 1 to 2 |
| 4. Services | `a2m-policy`, `a2m-auth`, `a2m-sources` | Layers 1 to 3 |
| 5. Contract | `a2m-provider` | Layers 1 to 4 |
| 6. Engine | `a2m-engine`, `a2m-sched` | Layers 1 to 5 |
| 7. Actions | `a2m-actions` | Layers 1 to 6 |
| 8. Roles | `a2m-runner`, `a2m-mcp`, `a2m-api`, `a2m-local` | Layers 1 to 7 |
| 9. Pack bases | `a2m-pack-web-browser` | Layers 1 to 5 |
| 10. Packs | `a2m-pack-web-ui`, `a2m-pack-web-probe`, `a2m-pack-http`, `a2m-pack-mcp`, `a2m-pack-desktop`, `a2m-pack-command`, `a2m-pack-data`, `a2m-pack-file`, `a2m-pack-stream`, `a2m-pack-device` | Layers 1 to 5, and the base of its own pack |
| 11. Product | `anything2mcp` | Everything |
| 12. Copy test harness | `a2m-copy-test-harness` | Everything |

Three rules follow from the table, and the audit states each one by name when it is broken:

- **A kernel crate never depends on a pack crate.** The engine reaches a driver through the registry in `a2m-provider`, never by naming a pack.
- **A pack crate sees the kernel up to the contract, and nothing above it.** It cannot call the engine, the scheduler, the action registry, or a front door. It registers what it offers and is called.
- **A pack crate depends on another pack crate only when that crate is the base of its own pack.** The pack is the third word of the name: `a2m-pack-web-ui` may use `a2m-pack-web-browser`, and `a2m-pack-http` may not.

Only the product crate names packs, and it does nothing else with them than hand them to the registry.

## Core types

The crate `a2m-core` holds the words every other crate shares. A word with a fixed set of values is an enum whose names are the ones in the documents; a test in the crate reads the documents and fails when the two differ, in content or in order. Each enum reads and writes its values by name, in files, in the ledger, and in JSON.

| Type | Values | Defined in |
| --- | --- | --- |
| `Archetype` | Seven, fixed | [Spec section 201](../spec/2-kernel/201-archetypes-and-modes.md) |
| `EffectClass` | Three, ordered from least to most serious | [Spec section 212](../spec/2-kernel/212-policy-and-approvals.md) |
| `Hazard` | Five, ordered from least to most serious | [Spec section 213](../spec/2-kernel/213-effects-hazards-and-consent.md) |
| `FailureClass` | Twelve | "Failure codes" below, and [spec section 207](../spec/2-kernel/207-the-engine-loop.md) |
| `EvidenceLevel` | Seven, ordered as the ladder; the default is the weakest | [Spec section 219](../spec/2-kernel/219-the-evidence-ladder.md) |
| `WriteEvidence` | Three: `read_back`, `response`, `acknowledged`; what a write binding offers as the proof that its write landed, and only an operation declared `irreversible` may offer `acknowledged` | [Spec section 208](../spec/2-kernel/208-runs-the-ledger-and-resume.md), [decision 0161](../decisions/0161-the-plan-and-the-engine-after-the-packs-were-built.md) |
| `Coverage` | Three; the default is `unknown` | [Spec section 218](../spec/2-kernel/218-discovery-and-findings.md) |
| `RunState`, `RecordState` | Twelve and six | [Spec section 208](../spec/2-kernel/208-runs-the-ledger-and-resume.md) |
| `SessionState` | Eight | [Spec section 215](../spec/2-kernel/215-sessions-and-the-session-keeper.md) |
| `BindingStatus` | Four | [File formats](file-formats.md) |
| `Source` | Five; each has the strongest level a finding from it can have | [Spec section 218](../spec/2-kernel/218-discovery-and-findings.md) |
| `LimitationCode` | Eight | [Spec section 218](../spec/2-kernel/218-discovery-and-findings.md) |
| `UnknownReason` | Four | [Spec section 218](../spec/2-kernel/218-discovery-and-findings.md) |
| `Availability` | Eight; the default is `unknown`, which is never `ready` | [Spec section 203](../spec/2-kernel/203-the-provider-contract.md) |
| `SubsystemState`, `SubsystemMove` | Four and three | [Spec section 204](../spec/2-kernel/204-supervising-subsystems.md) |
| `DoctorState` | Seven; the default is `unknown` | [Spec section 204](../spec/2-kernel/204-supervising-subsystems.md) |
| `IdentityStrategy` | Five | [Spec section 209](../spec/2-kernel/209-record-identity-and-dependent-records.md) |
| `FieldBase`, `Sensitivity`, `EnvironmentKind`, `TestKind`, `Checkpoint` | Ten, three in order, two, two, two | [File formats](file-formats.md) |
| `SignInMode`, `DriftRisk`, `WhitespaceRule`, `DecimalMark` | Three, three in order, three, two | [File formats](file-formats.md) |
| `CaptureLevel` | Three, each including the one before | [Spec section 217](../spec/2-kernel/217-traces-evidence-and-repair.md) |
| `ApproveWrites` | Three, ordered from least to most strict | [Spec section 226](../spec/2-kernel/226-configuration.md) |
| `Rounding` | Seven: `half_up`, `half_even`, `half_down`, `up`, `down`, `ceiling`, `floor` | [Decision 0068](../decisions/0068-values-names-and-identifiers-as-the-core-crate-settles-them.md) |
| `DigestPurpose` | Six: `blob_name`, `restricted_value`, `record_key`, `caller_key`, `watch_value`, `shadow_input` | [Decision 0069](../decisions/0069-the-plan-is-canonical-json-hashed-whole-and-keyed-digests-are-per-workspace.md), [decision 0262](../decisions/0262-watches-are-held-to-their-end-their-budget-and-their-runs-own-result.md) |
| `AddressScope` | Twelve; what kind of place an address is, read from its literal alone | "Where an address is" below, and [decision 0240](../decisions/0240-one-typed-reading-of-where-an-address-is-and-one-question-of-a-plain-link.md) |

Some words are checked names and not enums, because their values arrive with packs, crates, and target files ([decision 0060](../decisions/0060-a-mode-is-a-registered-name-and-the-archetype-is-the-closed-type.md), [decision 0062](../decisions/0062-words-that-arrive-with-packs-are-registered-names-and-packs-share-data-never-code.md)):

| Type | Shape | Who supplies the values |
| --- | --- | --- |
| `Mode` | Lowercase letters, digits, and underscores, starting with a letter; at most 32 characters | A provider registers each mode it brings, with its archetype |
| `FailureCode` | The same shape; at most 64 characters | Each crate names the codes it can produce; a code never changes once released |
| `SurfaceKind`, `StepKind`, `Verb`, `FileKindName`, `FindingKind`, `ControlName` | The same shape; at most 32 characters | A provider registers each one it brings. A step kind is a name within its mode |
| `AuthProviderName` | The same shape; at most 32 characters | The auth providers handed to the session keeper |
| `CredentialRef` | `vault://` and one to four parts joined by `/`, each of lowercase letters, digits, `_`, `-`, and `.`. A refusal never shows what was written | The target's `auth.yaml`; the vault resolves it |
| `LintCode` | An owner and a rule, each of that shape, joined by one dot; at most 64 characters | `kernel`, or the provider that registers the rule |
| `TargetName`, `SurfaceName`, `Environment` | The same shape; at most 32 characters | The target's files |
| `OperationName`, `AccountName` | The same shape; at most 64 characters | The target's files, and the settings. The compiler checks the parts of a tool name |

These complete the crate ([decision 0068](../decisions/0068-values-names-and-identifiers-as-the-core-crate-settles-them.md), [decision 0069](../decisions/0069-the-plan-is-canonical-json-hashed-whole-and-keyed-digests-are-per-workspace.md)); the [crate's design](../architecture/crates/a2m-core.md) has every signature:

- **`Failure`** is a class, a code, a plain sentence, and a next step. The sentence and the next step are shown to callers and must never contain a secret. A name that is not one of a vocabulary's values, or is badly formed, becomes a failure of class `input` with the code `schema_invalid`, and its next step lists the values that would have been accepted.
- **`Finding`** is what discovery returns. It is made only through its builder, which refuses a level above `discovered`, above what the source supports, or above the weakest finding it was derived from. What is not stated takes the weakest value. Its identifier is a `Digest` of its content, so the same observation has the same identifier everywhere, and a stored finding is checked against its identifier when it is read. Its `Subject` is one of six kinds; an address subject carries no credentials, query, or fragment.
- **`Secret`** wraps a value that must never be shown. Printing it gives `[redacted]`. It cannot be serialized or copied, and the build fails if it ever can. The value is reached only through a method named `expose`, so that every use can be found. Its bytes are overwritten with zeros when it is dropped ([decision 0122](../decisions/0122-small-things-the-core-crate-gives-its-builders-and-a-secret-is-wiped-when-dropped.md)).
- **A checked name is declared with one macro,** `checked_name!`, which any crate may use for a name of its own. Each such type has `fixed`, which makes a name written in the source and fails the build when the name is badly formed. A badly formed name includes one Windows keeps for a device (`con`, `prn`, `aux`, `nul`, `com0` to `com9`, `lpt0` to `lpt9`), alone or as a part of a dotted name, on every system; `InvalidName::kept_for_a_device` names that part, and its message and `next_step` say so ([decision 0431](../decisions/0431-a-name-that-windows-keeps-for-a-device-is-refused-on-every-system.md)).
- **`Value`** is what a field holds: nothing, true or false, a whole number, text, a decimal, a moment, a date, a length of time, a list, or a record. It is never a float and never a secret, and kinds are never converted. **`Values`** holds named values sorted by name, so the same values have the same bytes and the same digest whoever built them. **`ValueType`** describes a value's type, for the expression checker and the compiled target alike. **`Decimal`** is exact to 28 significant digits: a result that does not fit is refused, never rounded, and a division takes places and a rounding mode. It is compared, hashed, and written in canonical form by its exact text, `exact_text`, and otherwise written with the places it holds; `with_places` pads it to the places a result's field type states, `CompiledFieldType::result_places`, and never rounds ([decision 0444](../decisions/0444-a-results-decimal-is-padded-to-the-places-its-field-type-states.md)). **`RecordKey`** is one or more plain values in declared order.
- **Identifiers** are 16 bytes, ordered by the moment they were made, written as a one-letter prefix, an underscore, and 32 hexadecimal characters: `RunId` (`r_`), `JobId`, `SessionId`, `ApprovalId`, `DecisionId`, `WorkspaceId`, `PrincipalId`, `RunnerId`. The crate reads no clock and no random source; whoever makes one brings both. **`Identity`** is a person or a named account, never a credential.
- **`CompiledTarget`** is the compiled target as data, with `encode`, `decode`, `check`, and `show`. It carries everything the run time needs from a target's files: beside the surfaces, field types, operations, pipelines, tools, and tests, the target's own facts, each environment's test records and approval rule, and how each group of surfaces is signed in to, with credentials as references only ([decision 0121](../decisions/0121-the-plan-carries-what-the-run-time-needs-from-target-files.md)). It is encoded as canonical JSON after one line that names the format; the hash is the digest of all the bytes, and `decode` checks it before it reads anything. A pack's own data is a `PackPayload`, held as canonical JSON text that the kernel never parses.
- **`Redact`** and **`KeyedDigest`** are the two interfaces the vault implements. `Redact` turns text into text with every registered secret removed, and redacts a whole failure or a set of values in one call. A keyed digest is separate for each workspace and each purpose, and has one construction, HMAC-SHA-256.

### Where an address is

**`ScopedHost`** reads a host from its literal alone, with no name lookup and no system call, and keeps its **`AddressScope`**, its IP address, its name as written, whether it was written as IPv4 inside IPv6, and whether it carried a zone. It reads an IP address, a socket address, a URL's host, or a destination's address (a URL, `host:port`, `[v6]:port`, or a bare host). An IPv4 address inside IPv6 is read as the IPv4 address it means. **`ScopedHost::may_leave_this_machine`** is the one question every check of a plain link asks: it is false only for a loopback address written plainly as an IP address, so a name (`localhost` included), IPv4 inside IPv6, and an address with a zone may all leave. `a2m-auth`'s OAuth endpoints, the HTTP and MCP packs' plain `http` links, the stream pack's plain TCP, and the data pack's server store ask it after their own parsers ([decision 0240](../decisions/0240-one-typed-reading-of-where-an-address-is-and-one-question-of-a-plain-link.md)). The ranges are those the Rust core library classifies, with the standards it cites.

| Scope | Meaning |
| --- | --- |
| `this_machine` | A loopback address: `127.0.0.0/8` or `::1`. Never a name |
| `unspecified` | `0.0.0.0` or `::` |
| `link_local` | `169.254.0.0/16` or `fe80::/10` |
| `private_use` | `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16` |
| `carrier_grade_nat` | The shared address space, `100.64.0.0/10` |
| `unique_local` | `fc00::/7` |
| `multicast` | `224.0.0.0/4` or `ff00::/8` |
| `documentation` | `192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`, `2001:db8::/32`, `3fff::/20` |
| `reserved` | Another special-purpose range that is not globally reachable |
| `public` | In no special-purpose range the reading holds |
| `name` | A host name, kept as written and never resolved here |
| `unreadable` | Malformed, a spelling only a lenient parser widens (`127.1`), or not a network address at all |

## Driver interface

The driver interface, the manifest, the registry, and the supervisor are built in `a2m-provider`. The [crate's design](../architecture/crates/a2m-provider.md) gives the interface in full, and decisions [0088](../decisions/0088-the-provider-contracts-outside-crates.md), [0089](../decisions/0089-the-provider-contract-as-building-it-settled-it.md), and [0090](../decisions/0090-the-supervisor-ends-a-group-only-while-its-leader-is-unreaped.md) record how building it settled it. The main types:

- **`Driver`** is what a pack implements for its modes, with the four calls below.
- **`DriverHandle`** is the only way the engine reaches a driver. The registry gives one for a mode, and it checks every step on the way in and every outcome on the way out.
- **`Authority`** is what a step is run under: the right to read, or the intent token for one write.
- **`Destinations`** are the addresses a run may reach: the compiled target's surfaces and the operation's manifest, compared as written. Each carries the scope read from its own address when the set is made (`Destinations::scope_of`); nothing else sets it, and it decides nothing yet ([decision 0240](../decisions/0240-one-typed-reading-of-where-an-address-is-and-one-question-of-a-plain-link.md)). `Destinations::place_of(address, &MachineState)` derives its **network place** from that scope and from the subnets of the machine's own interfaces that are up: `this_machine`, `local_network`, `public_internet`, `via_gateway` (never derived until a person can name a gateway), or `unknown`, which is every name, every address that cannot be told, and every address when the interfaces cannot be read. A place is never set by an input and no name is resolved ([decision 0313](../decisions/0313-the-place-of-an-address-is-derived-from-its-scope-and-the-machines-subnets.md)).
- **`StepInput`** is what one step is given, with its values already evaluated. **`StepOutcome`** is how a step ended when it did not fail.
- **`StepFailure`** is a failure together with **`Sent`**, which says whether the request was not sent, was answered, or is unknown. What the system itself said travels on the failure, apart from its sentence (`StepFailure::with_said`).
- What a record's earlier steps recorded reaches a driver while it acts, as **`Prepared::recorded`**; how a step was done is the capture **`Capture::acting`**; the run's project folder is **`CheckedPaths::project_folder`**; a verified save in steps is **`CheckedPath::create_temporary`** and **`Temporary`** ([decision 0162](../decisions/0162-the-provider-contracts-second-pass-what-a-step-is-handed-and-hands-back.md)).
- A root bound to no folder, which stands for each request's project folder, is **`Roots::add_project`**; **`Roots::check_in`** and **`Roots::folder_in`** resolve it in the run's project folder ([decision 0193](../decisions/0193-the-core-the-contract-the-compiler-and-the-scheduler-after-the-second-pass.md)).
- A path resolved in the one form every root holds its folder in (on Windows the ordinary form `C:\...` where it names the same thing) is **`resolved_path`** of `a2m-provider`; whatever compares a path with a root's folder resolves it by this and by nothing else.
- **`Prepared::recorded_elsewhere`** asks the engine whether another record holds a value, and answers `Held`, `NotHeld`, or `CannotTell`, never the value ([decision 0193](../decisions/0193-the-core-the-contract-the-compiler-and-the-scheduler-after-the-second-pass.md)).
- **`SurfaceNotice`** (in `a2m-core`) is a closed list of the notices a door must show with a surface, such as unencrypted credentials; a kind registers one with `SurfaceKindSpec::with_notice`, and `CompilerSet::surface_notices` computes them ([decision 0193](../decisions/0193-the-core-the-contract-the-compiler-and-the-scheduler-after-the-second-pass.md)).
- **`ActionContext::paths`** is the call's own checked project folder, handed to a provider action; `DiscoveryScope::paths` is the run's ([decision 0196](../decisions/0196-the-engine-and-the-registry-after-the-acknowledgement-and-the-first-hour.md)), with each file a person handed the discovery, under the name a discoverer declares for it ([decision 0263](../decisions/0263-a-person-hands-the-scout-files-of-the-projects-sources-folder-by-names-the-discoverers-declare.md)).
- The closed **shape** of a body is the module **`shape`** of `a2m-provider`: **`Shape`**, read from a target file by `Shape::from_doc` in the seven words `type`, `nullable`, `properties`, `required`, `additionalProperties`, `items`, and `enum`, any other refused where it is written with a **`ShapeProblem`**; **`Kind`**, the words of `type`; and `Shape::check`, which checks any value that implements **`ShapeValue`** (built in for `serde_json::Value`) and gives a **`Misfit`**, where and how a value does not fit, never the value. The HTTP pack's endpoint entries and the web pack's page requests read their shapes through it, and a pack may take fewer of the words, never more ([decision 0394](../decisions/0394-an-asserted-read-of-a-web-surface-is-a-page-request-matched-by-one-shared-shape-and-shown-at-publish.md), point 1; [decision 0404](../decisions/0404-one-closed-shape-of-a-body-in-the-provider-contract-and-the-stricter-refusal-where-the-two-copies-differed.md)).
- **`DraftContext::open_question_quoting`** makes an open question whose sentence is the product's and whose quoted words are the system's, kept apart ([decision 0193](../decisions/0193-the-core-the-contract-the-compiler-and-the-scheduler-after-the-second-pass.md)).

- **`Replay`** is a driver of recorded answers that every pack gets from the contract, so that an offline test runs the tool's steps against a fixture; `Registry::replay_driver` hands it out and keeps the pack's own step kinds, commit kinds, and codes ([decision 0177](../decisions/0177-a-targets-offline-tests-run-against-a-replay-of-their-fixtures.md)). `Replay::sent` gives what the engine sent to each committing step, for a test to compare with the request a fixture states, and `RecordedAnswer::repeats` says that one answer serves every step of its name ([decision 0180](../decisions/0180-a-targets-offline-tests-cover-writes-and-gate-the-publish.md)).
- **`RefusedProvider`** is a provider whose manifest was refused, kept by `Registry::refused` so that `provider.list` names it with its problem ([decision 0176](../decisions/0176-the-action-registry-takes-up-the-kernels-second-pass.md)).

The plug-in host and the framed link are not built yet.

| Call | Does |
| --- | --- |
| `prepare` | Acquires what the steps need: a page, a client, a connection, a link |
| `act` | Carries out one compiled step. Confirming and reading back are steps like any other, so there is no separate read-back call |
| `health` | A cheap check used by canaries and by the choice of binding |
| `release` | Gives back what `prepare` acquired |

Rules for every driver:

- No retries inside `act`. The engine decides what is tried again.
- Every failure maps to exactly one failure class, and says whether the request was not sent, was answered, or is unknown.
- No secret ever appears in a step's outcome or in a failure.
- A driver is reached only through a handle that refuses a step that commits without the token the ledger gives for a recorded intent.
- A step marked as a lookup may report that a record is absent. No other step may. "Absent" proves that a record with no key is not there only from steps whose kinds the mode lists in `ModeDecl::absence_steps` (empty by default); `DriverHandle::proves_absence(steps)` asks it ([decision 0185](../decisions/0185-a-record-with-no-key-is-never-proved-absent-by-one-look.md)).
- It must be safe to drop the future of `prepare`. The engine runs it under the time the mode's manifest states, `ModeDecl::prepare_within_ms`, which `DriverHandle::prepare_within()` gives and the manifest check holds between one millisecond and **`PREPARE_CEILING`** (ten minutes), and under `EngineSettings::prepare_timeout` (30 s) where the manifest states none; past it the preparation is dropped, `transient`/`prepare_timed_out` ([decision 0427](../decisions/0427-a-mode-states-how-long-its-prepare-may-take-and-the-engine-honours-it-under-a-ceiling.md)).
- A step's outcome can carry values known before the commit point, such as a key the store assigned. The engine records them before the committing step runs.
- A failure of a committing step that was answered with class `policy` or `input` leaves the handle as `possible_commit`, `no_confirmation`, and `Sent::Unknown`: those two classes say nothing was sent, and on a write that left they are not the system's verdict ([decision 0148](../decisions/0148-the-provider-contract-and-the-scheduler-after-their-safety-audit.md)).
- The handle refuses a step that commits and is not marked, where the mode's manifest lists its kind in `commit_steps` (`policy`, `intent_missing`), and it lets no driver return a code in `codes::KERNEL_ONLY`, seven codes that state the kernel's bookkeeping. It redacts what a step returns, its values, its values for reconcile, and a capture whose bytes are text, as it does a failure.
- `CheckedPath` writes under a random name, only where nothing stands, and never through a link; the new methods are `entry_kind` (what stands at the final name, a link reported and not followed) and `create_dir` (one folder, new; it fails when anything is at the name). A link at the final name is refused for `write` and `remove_file`. A pack's manifest may not name a program by a relative path.
- **`EntryKind`** is what stands at a name: `Absent`, `File`, `Folder`, `Link`, `Other`, or **`NotOnThisMachine`**, a regular file whose bytes are not on this machine: on Windows one with the attribute of recall on data access, of recall on open, or offline, and on macOS one with `SF_DATALESS`; Linux reports none. The marks are read with the entry's type, without opening it. **`EntryKind::of_metadata`** gives the kind of what `std::fs::symlink_metadata` said of an entry, by the same rule, for a pack that lists a folder itself. `CheckedPath::open`, `read`, `write`, `create_temporary`, and `may_write`, and `Temporary::replace`, refuse such a file before anything is read or opened, `policy`/`file_not_on_this_machine`; `size` and `entry_kind` read nothing of it and give its answer ([decision 0430](../decisions/0430-a-file-whose-bytes-are-not-on-this-machine-is-refused-by-name-and-never-fetched.md)).
- The supervisor digests the program it opens at every start and refuses a digest that is none of the declared ones; on Linux it starts the file it digested.

The check of who may change a program, and what it gives the supervisor, are in the contract ([decision 0250](../decisions/0250-one-check-of-who-may-change-a-program-and-the-mcp-packs-local-server-on-windows.md)):

- **`check_program(path, &ProgramTrust, ProgramRole)`** walks the program's path and refuses it as a **`ProgramRefused`** when another user could change it or a folder on the way to it. **`ProgramTrust::with_groups`** names the groups whose write counts as the administrator's. **`ProgramRole`** is `Program` (a program that takes its arguments as data), `Interpreter` (the interpreter of a fixed pair), `OwnCode` (a program of the closed list `runs_its_own_code`, started only in its listed forms), `Script` (a file the engine wrote), or `Folder` (a folder it made); the last two are walked like a program's path without its name and first-bytes checks.
- **`is_shell_or_interpreter`** keeps the list of shells and interpreters once. **`reads_its_line_as_code`** names the interpreters that read their whole command line as code. **`interpreter_form`** holds an interpreter and its fixed arguments to the closed table of forms, and refuses the rest as an **`InterpreterRefused`**. **`runs_its_own_code`** gives, for a program the name list refuses, its row of the closed list of commands that run no code but its own: an **`OwnCode`** with its **`OwnCodeKind`** (`Launcher`, `VersionManager`, `ContainerRunner`), its names, and its **`OwnCodeForm`**s, each the exact elements and whether `--` and values may follow ([decision 0349](../decisions/0349-the-two-steps-of-decision-0299-as-built-a-closed-list-of-commands-and-the-javascript-runtime-as-a-launcher.md)).
- **`redirects_loading`** names the variables that make a program load other code. The supervisor refuses a launch that names one, `policy`, `variable_refused`, for every pack.
- **`LinkKind::Standard`** is a link over the child's standard input and output, started by **`Supervisor::start_on_standard_streams`** or **`ScopedSupervisor::start_on_standard_streams`**, on every system; its standard error goes to the null device.
- **`CheckedPath::create_private_dir`** and **`create_private_dir_all`** make a folder its user's alone from the moment it exists: mode `0700` on Linux and macOS, and on Windows a protected access list for the folder's owner, the local system account, and the Administrators group.

A folder's writes act through the folder held from the root down ([decision 0272](../decisions/0272-the-file-packs-folder-writes-act-through-a-held-folder-after-a-second-reader.md)):

- **`CheckedPath::hold_folder`** gives a **`HeldFolder`**: the folder, reached from the root down with no link followed. Each of its calls (`kind`, `identity`, `names`, `create_new`, `link`, `remove_file_if`, `remove_empty_folder`, `sync`) names one entry directly in it and follows no link at that name, and gives the system's own `std::io::Error`. On Linux and macOS the held folder is its own descriptor; on Windows every folder from the root down is held open, and none of them can be renamed, removed, or replaced by a link while it is held. **`HeldFolder::still_at_its_path`** says whether the folder reached afresh from the root is still the one held.
- **`HeldFolder::remove_file_if`** removes a file only where it is the file expected and the caller's check of its bytes, read through a handle of that file, says yes. It answers a **`Removal`**: `Removed`, or `NotThatFile { put_back }`.
- **`FileIdentity`** is the system's identifier of a file: the device and the file number on Linux and macOS, and on Windows the volume's serial number and the file's identifier. **`FileIdentity::to_bytes`** gives it as 24 bytes, for a digest that tells one file from another; it is never shown.

A discovery is handed files by name ([decision 0263](../decisions/0263-a-person-hands-the-scout-files-of-the-projects-sources-folder-by-names-the-discoverers-declare.md)):

- **`DiscovererDecl::files`** lists the names under which a discoverer reads a file a person hands a discovery, from the manifest's `files`; empty by default. Each is lowercase letters, digits, and underscores, starting with a letter, at most 32 bytes.
- **`Registry::files_taken`** gives every name that the discoverers suiting a surface declare.

Traffic capture is a capability a pack may offer ([decision 0391](../decisions/0391-traffic-capture-is-a-capability-of-the-provider-contract-that-yields-structure-only.md)):

- **`points.traffic_capture`** in a manifest lists the surface kinds whose traffic the pack captures; empty by default, and a pack that lists none is asked nothing. **`Registrar::traffic_capture`** takes the pack's one **`CaptureSource`**; registering one with no kind listed is `policy`/`effect_not_declared`.
- **`CaptureSource::open(surface, deadline)`** gives a **`TrafficCapture`**: `start` begins recording, and `take_recorded(until)` gives what was recorded since the last take as **`CapturedTraffic`**: the **`CapturedRequest`**s, oldest first, and how many were `dropped`, `awaited`, and `withheld`. A `CapturedRequest` is structure only: `method`, `origin`, `path` (a segment that is not kept is `{value}`, **`CAPTURE_PLACEHOLDER`**), the `query`'s names and `query_left_out`, `credential_headers` and `set_headers` by name and `headers_left_out`, `status`, `media_type`, and `fields`, each name with one of **`FIELD_KINDS`**, and `names_withheld`; and of the body the request sent, `body_media_type`, `body_fields`, and `body_names_withheld`, by the same rule (decision 0392, point 2).
- **`take_capture(capture, until, redact)`** is the one way a yield is read: it applies **`kept_by_structure`**, which withholds a request whose method or origin holds a secret or is not of its form, and holds every path segment, name of a query's parameter or a header, and part of a field's name to **`is_captured_word`**: at most **`LONGEST_CAPTURED_WORD`** (32) characters of ASCII letters, digits, `_`, `-`, and `.`, at most **`MOST_CAPTURED_DIGITS`** (3) digits, and no run of hexadecimal digits longer than **`LONGEST_HEX_RUN`** (15). A segment that fails, or holds a secret, is `{value}`; such a name is dropped, and the request says one was left out. A body's names and its media type pass the check an answer's pass.
- **`Registry::captures_for(kind)`** gives a **`CaptureHandle`** for each pack whose manifest lists the kind. **`CaptureHandle::open`** refuses a surface of another kind, `policy`/`effect_not_declared`, before the pack is asked, and gives a **`Capturing`**, whose `take` is `take_capture` with the host's redactor.

The web pack's base offers a capture of a person's own use in a **watching tab** ([decision 0392](../decisions/0392-a-page-that-reads-by-a-method-that-may-write-is-learned-from-a-persons-watched-use-and-read-through-an-asserted-request.md), point 1; [decision 0411](../decisions/0411-the-watching-tab-records-from-the-browsers-network-events-and-keeps-each-screen-without-its-values.md)): **`Observer::watching_tab(profile, env, notice, origins, start, deadline)`** opens a window, as the sign-in does, with a notice that says what is kept, and gives a **`WatchTab`**, sent once to a **`WatchStart`** (an address on the origins that does not look like an action, else `surface_not_declared` or `address_looks_like_an_action`) and nowhere after. It has no method that presses, types, navigates, or sends; it fails no request for its method, holds its documents to the origins, and implements `TrafficCapture`, recording from the moment it opens every request a script, a beacon, or a posted form sends to those origins, with its body's media type and its fields' names and kinds up to **`LONGEST_WATCHED_BODY`** (64 KiB). **`WatchTab::screens`** gives **`WatchedScreens`**: a snapshot of each screen the page came to, with no element's value but a password's mark, taken once the page is idle or **`SETTLE_BOUND`** (10 s) has passed, at most **`SCREENS_KEPT`** (40). `changes`, `changed_since`, `looked_at`, `looked_at_since`, `is_open`, and `closed` are waits.

A discoverer may be driven by a person, over a capture ([decision 0392](../decisions/0392-a-page-that-reads-by-a-method-that-may-write-is-learned-from-a-persons-watched-use-and-read-through-an-asserted-request.md), point 2, as [decision 0395](../decisions/0395-a-person-driven-discoverer-is-called-through-a-method-of-its-own-and-the-scope-is-left-as-it-is.md) builds it):

- **`DiscovererDecl::driven_by`** is a **`DrivenBy`**, `engine` or `person`, from the manifest's `driven_by`; `engine` when left out. A manifest with a discoverer driven by a person and no `points.traffic_capture` is refused, `policy`/`effect_not_declared`.
- **`Discoverer::watch(scope, end, sink)`** is how a discoverer a person drives is run: it is handed no port, reads its pack's capture, and returns when its **`CaptureEnd`** is set. Its default answer is `unknown_variant`/`discoverer_cannot_watch`, so a discoverer the engine drives implements nothing more. A `CaptureEnd` is asked with `is_set` or waited on with `ended`; nothing but the contract sets it.
- **`Registry::capture_stop`** gives a **`CaptureStop`**, the kernel's half: `stop` sets the end, `end` gives the `CaptureEnd` to hand over, and its clones stop the same capture.
- **`DiscovererHandle::watch(scope, end, sink)`** is the only caller of `Discoverer::watch`, and `DiscovererHandle::discover` the only caller of `discover`; each refuses, `policy`/`effect_not_declared`, a discoverer whose `driven_by` names the other. `watch` checks each finding against the manifest as `discover` does. It returns at the end, or at its bound: the earlier of the scope's deadline and its `seconds`, held to the manifest's, where the handle sets the end itself and the report's `bounds_hit` names `seconds`. Once the end is set, a discoverer that has not returned within **`CAPTURE_END_GRACE`** (5 seconds) is given up, `transient`/`timeout` ([decision 0406](../decisions/0406-a-person-driven-discoverer-is-held-to-its-bound-by-the-handle-and-has-a-code-for-the-watch-it-lacks.md)).
- **`Registry::discoverers_for`** gives only discoverers the engine drives, what a scout or `discover.run` may start, and **`Registry::files_taken`** counts only theirs. **`Registry::capture_discoverers_for`** gives only those a person drives, for a capture job.

## Verbs by mode

| Verb | `ui` | `page_http` | `http` | `api` | `site_tools` | `mcp` | `desktop` |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `open` | Yes | No | No | No | No | No | Yes |
| `read` | Yes | Yes | Yes | Yes | If offered | If offered | Yes |
| `list`, `search` | Yes | Yes | Yes | Yes | If offered | If offered | Yes |
| `create`, `update`, `upsert` | Yes | Yes | Yes | Yes | If offered | If offered | Yes |
| `delete` | Yes | Yes | Yes | Yes | If offered | If offered | Yes |
| `upload` | Yes | Yes | Yes | Yes | No | No | Yes |
| `download` | Yes | Yes | Yes | Yes | No | If offered | No |
| `submit` | Yes | Yes | Yes | Yes | If offered | If offered | Yes |
| `call` | No | Yes | Yes | Yes | Yes | Yes | No |

The columns are the modes of the first four packs. Every further pack adds a column in its own design: the command, data, file, stream, and device packs each state the verbs they offer and how each lowers into steps.

The verbs each built pack registers, as its manifest declares them. A pack's design says how each lowers into steps.

| Mode | Pack | Verbs registered |
| --- | --- | --- |
| `http` | `a2m-pack-http` | `read`, `list`, `search`, `create`, `update`, `upsert`, `delete`, `submit`, `call` |
| `cli` | `a2m-pack-command` | `read`, `list`, `search`, `create`, `update`, `delete`, `call` |
| `sql` | `a2m-pack-data` | `read`, `list`, `search`, `create`, `update`, `delete` |
| `file` | `a2m-pack-file` | `read`, `list`, `update`, `create`, `delete` |
| `stream` | `a2m-pack-stream` | `read`, `list`, `await`, `publish`, `take` |
| `device` | `a2m-pack-device` | `read`, `update`, `submit` |
| `mcp` | `a2m-pack-mcp` | `read`, `list`, `search`, `create`, `update`, `upsert`, `delete`, `download`, `submit`, `call` |

The `http` column above also offers `upload` and `download`; the HTTP pack does not register them yet.

## Widget drivers and platform profiles

Both are the web pack's knowledge and live in its section: [widget drivers](../spec/3-packs/301-the-web-pack.md#widget-drivers) and [platform profiles](../spec/3-packs/301-the-web-pack.md#platform-profiles).

## Auth provider interface

Each provider implements five operations: `sign_in`, `is_signed_in`, `detect_expiry`, `refresh`, `sign_out`, plus `inject`, which places credentials into a driver's context without returning them. Only the session keeper calls a provider. `recommend(findings) -> Recommendation` of `a2m-auth` is the fixed table `auth.detect` uses: it reads `sign_in_method` findings and gives choices of mode and provider with reasons, questions, and what is unknown, always marked `inferred` ([decision 0367](../decisions/0367-auth-detect-recommends-a-mode-and-a-provider-by-fixed-rules-and-marks-it-inferred.md)). A provider also states what it needs and can do (a person, a silent renewal, a light prompt, state that can move, and whether a successful exchange confirms the credential) and checks an `auth.yaml` entry without signing in. The traits as built are in the [crate design](../architecture/crates/a2m-auth.md).

Every outbound HTTP client of the product is made by the module `tls` of `a2m-auth`: `tls::client_builder` takes a `Trust` (the roots of `network.trust_roots`, then the operating system's) whose TLS configuration states its provider, and `tls::refusal` names a certificate or a handshake that failed; the design of [`a2m-auth`](../architecture/crates/a2m-auth.md) has the signatures ([decision 0157](../decisions/0157-tls-is-switched-on-with-the-provider-stated-in-each-client.md)). A provider lives in `a2m-auth` when it only holds or exchanges a credential. It lives in a pack when it needs a surface of the system. A provider that needs a person's consent in a browser, and is not in the web pack, gives the person a link to open in their own browser; nothing drives that browser.

| Provider | Covers | Lives in |
| --- | --- | --- |
| `sso_oidc`, `sso_saml` | Entra ID, Okta, Google Workspace, ADFS, Ping | `a2m-pack-web-ui` |
| `form_login` | Username and password pages | `a2m-pack-web-ui` |
| `os_broker` | Windows Web Account Manager, macOS Enterprise SSO, Linux identity broker; user devices only | `a2m-auth` |
| `obo_token` | OAuth On-Behalf-Of exchange from the caller's token | `a2m-auth` |
| `http_basic`, `http_digest` | Simple intranet systems. The provider holds the credential; the HTTP driver carries out the scheme | `a2m-auth` |
| `windows_integrated` | NTLM and Kerberos, in the same division of work | `a2m-auth` |
| `client_certificate` | Mutual TLS | `a2m-auth` |
| `api_key`, `bearer_static` | Interfaces with fixed keys | `a2m-auth` |
| `oauth_client_credentials`, `oauth_authorization_code` | Interfaces with OAuth | `a2m-auth` |
| `upstream_mcp` | Authorization to a system's own MCP server | `a2m-pack-mcp` |
| `totp` | One-time codes for dedicated accounts, where IT allows. A function another provider calls, not a provider of sessions | `a2m-auth` |

Further packs bring their own where a credential is bound to their kind of connection: the data pack brings a database password provider.

The web pack's sign-in through an application's own pages is built in `a2m-pack-web-browser`, `a2m-pack-web-ui`, and `a2m-auth` ([decision 0245](../decisions/0245-a-web-application-is-signed-in-to-through-its-own-sign-in-page-with-a-credential-stored-for-the-person.md)):

- **`form_login`** fills the application's sign-in page in the engine's own browser with a stored credential, tried once. Its `auth.yaml` settings name parts of `screens/` and no locator: `screen`, `user_field`, `pass_field`, `submit`, `refused`, and optionally `blocked`, `code_screen`, `code_field`, `code_submit`, `code_refused`, and `wait_ms`. Its credential roles are `user` and `password`, vault references under `vault://<target>/`; `attempt_limit` must be 1.
- **`Browser::export_state`** reads the cookies the browser holds for given origins as a **`BrowserState`** of **`StateCookie`**s, each value a secret; **`Browser::import_state`** places a state into the browser's context once for each mark. A state cannot be printed with its values and is not serializable.
- **`Browser::import_state`** takes the run's `Origins` as well, and refuses a state that holds a cookie for none of them, placing nothing ([decision 0254](../decisions/0254-a-person-signs-in-in-a-window-of-the-engines-own-browser-and-the-web-pack-stops-on-every-declared-sign-of-expiry.md)).
- **`Visibility::Window`** is a browser that shows a window, for a sign-in a person carries out themself; it is always started afresh, in the same engine-owned data directory and in-memory context as any other, and its first tab shows the engine's own notice naming the target and the person. **`Browser::sign_in_tab(env, look_on, pass_through)`** opens the tab a person signs in in: it is held to the application's origins and an identity provider's, and its helper looks at pages of the application's origins alone and reads nothing on the others (decision 0254).
- **`PageAnswer::withheld`** says that the answer of a write from inside a page held the page's anti-forgery token in some spelling and was withheld whole; the write is then in doubt.
- **`AuthEntry::surfaces`** is an **`EntrySurfaces`**: the surfaces the entry covers, the compiled target's pack sections for their kinds, the addresses the target declares, and whether the compiled target is a published version's. **`EntrySurfaces::of`** reads it from a compiled target, and **`EntrySurfaces::published`** marks it as taken from a published version's compiled target, which only the caller that loaded that compiled target says ([decision 0248](../decisions/0248-a-sign-in-goes-only-where-a-published-version-says-and-a-credential-belongs-to-its-target.md)). `form_login` refuses an entry that is not marked.

## Vault interface

The design of [`a2m-vault`](../architecture/crates/a2m-vault.md) gives the interface in full. The main types:

- **`Vault`** seals a plain item into a **`Sealed`** envelope under a **`SealContext`**, and opens it again only under the same context. The caller stores the envelope.
- **`KeyStore`** is the back end that holds the root key. `OsKeyStore`, `PassphraseKeyStore`, and `MemoryKeyStore` implement it. A root, and the passphrase route's salt through `PassphraseSalt::load_or_create`, is made once, under a lock that two engines starting together both take.
- **`KeyRecords`** is where wrapped keys persist, as one **`KeyRecord`** per key and **`Generation`**. `a2m-ledger` implements it.
- **`Redactor`** implements `Redact`, and **`VaultDigest`** implements `KeyedDigest`, both from `a2m-core`. A text that arrives in pieces is redacted through the redactor's **`RedactStream`**, never piece by piece.

## Logs and traces interface

The design of [`a2m-logs`](../architecture/crates/a2m-logs.md) gives the interface in full. The main types:

- **`TraceBuilder`** takes one **`StepRecord`** per step, redacts it, and ends as a **`FinishedTrace`**: the bytes and the trace signature. A trace that still holds a secret ends as a **`TraceRefused`**.
- **`LogsAndTraces`**, made by `start` from **`LogSettings`**, is the handle for spans, metrics, and the engine's own logs.
- **`Clock`** is how time is given, so that a test can fix it.

## Compiler registration points

The design of [`a2m-files`](../architecture/crates/a2m-files.md) gives the interface in full ([decision 0079](../decisions/0079-the-compilers-registration-points-and-the-file-formats-as-the-code-settles-them.md)). The main types:

- **`CompilerSetBuilder`** takes what each pack registers, under the pack's own name, and ends as a **`CompilerSet`**, which `compile` and `lint_one` are given.
- **`FileKind`**, **`Lowering`**, **`LintRule`**, and **`DraftingRule`** are the four traits a pack implements. A surface kind, as a **`SurfaceKindSpec`**, and a control name are registered as data. A lowering states, for each kind of step, whether it saves, as a **`StepCommits`** (`Never`, `Always`, `AsDeclared`), and whether it can change the system, as a **`StepChanges`** (`Never`, `AsDeclared`, `Can`; a kind it does not mark is `Can`, and the compiler refuses an operation whose effect is `read` that holds a step of a `Can` kind, `kernel.read_acts`; [decision 0307](../decisions/0307-what-the-evidence-level-is-for-and-what-a-review-shows-about-a-tools-evidence.md), point 4), the least effect a binding has by what it does, and which of the operation's outputs its steps may return as `null` because the system may hold no value for them (`absent_outputs`, default none; [decision 0305](../decisions/0305-an-output-that-may-be-absent-is-declared-so-in-the-plan-and-the-output-schema.md)). A compiled target's output, an **`CompiledOutput`**, carries `may_be_absent`, left out of the encoded compiled target when false, so a compiled target that declares none keeps its bytes and its format; a compiled target's input, an **`CompiledInput`**, carries `precondition` in the same way ([decision 0378](../decisions/0378-an-input-marked-precondition-is-sent-with-a-write-and-never-compared-with-its-read-back.md)); a file kind states whether a document describes a write, which the drafter never writes ([decision 0153](../decisions/0153-what-the-compiler-the-plan-and-the-redactor-hold-by-themselves-after-the-safety-audit.md)); and the reads a document asserts, `FileKind::asserts`, empty by default: each an **`AssertedRead`**, the entry's name, its method, its path, and its body's fields as **`AssertedField`**s (name, the word of its `type`, its `enum`), which the review summary lists as `asserted` with the fixed sentence `ASSERTED_NOTICE`, and the publish page shows after the sign-ins and before the tools; the web pack's `page_request` kind and the HTTP pack's `endpoint` kind answer ([decision 0426](../decisions/0426-the-publish-page-names-each-asserted-read-after-the-sign-ins-from-what-each-file-kind-says-it-asserts.md)).
- **`CompilerSet::address_key`** gives the key under which a surface kind states its address, so that `target.add` can write a surface without a pack's knowledge in the registry ([decision 0199](../decisions/0199-the-halves-left-for-each-other-are-joined.md)).
- **`Compiled`** is a compiled target with its encoded bytes and its hash. **`Diagnostics`** is every problem found, each a **`Diagnostic`** with its place and a fix.
- **`tool_digest`** gives a compiled tool's digest, the SHA-256 of one canonical JSON object of its name, description, input schema, output schema, and annotations, which a client reads as `anything2mcp/digest` in a listed tool's `_meta` and as `digest` from `tool.describe`; **`UNTRUSTED_MARK`**, `x-anything2mcp-untrusted`, is the key the compiler sets to `true` on each output of an untrusted field type in a tool's output schema ([decision 0460](../decisions/0460-a-tools-digest-is-of-its-definition-as-published-and-the-untrusted-mark-sits-on-the-output-field.md)).

## Store interface

The design of [`a2m-ledger`](../architecture/crates/a2m-ledger.md) gives the interface in full (decisions [0080](../decisions/0080-the-store-is-one-sqlite-file-and-a-crate-reaches-its-own-tables-through-the-transaction.md) and [0081](../decisions/0081-what-the-ledger-holds-as-data-the-token-doubt-locks-and-rule-three.md)). The main types:

- **`Store`**, opened from a **`StoreConfig`**, owns the database, the migrations, and the blobs. Its `write` and `read` each run one closure as one transaction on the store's own thread, and hand the closure a **`Tx`**.
- **`Ledger`**, **`Findings`**, **`Versions`**, and **`Audit`** are the ledger's own tables inside a transaction. **`Blobs`** keeps evidence, sealed.
- **`Tables`**, from `tx.tables` with an **`Owner`**, is how a crate above reaches its own tables and no others.
- **`Fence`** names a run and its lease number. **`IntentToken`** is what the store gives for a stored intent, and what a step that commits must carry.

The ledger's shadow evidence says what it covers ([decision 0251](../decisions/0251-the-evidence-for-promoting-a-shadow-binding-says-what-it-covers-and-a-shadow-costs-what-it-says.md)):

- **`ShadowComparison`** carries `input`, the call's inputs, of which the ledger keeps only the workspace's keyed digest, and `requests`, what the shadow read cost in the rate budget's units.
- **`ShadowSubject`** carries `against`, the digest of the verified binding that acts beside the one in shadow, and `environments`, those the evidence must come from.
- **`EnvironmentEvidence`** is what one such environment holds toward a promotion: its distinct inputs and matches since the last mismatch. **`EnvironmentStanding`** is what `shadow.report` shows for a binding in one environment: reads, requests, matches, and distinct inputs.

## Policy interface

The design of [`a2m-policy`](../architecture/crates/a2m-policy.md) gives the interface in full ([decision 0083](../decisions/0083-the-policy-engines-interface-as-building-it-settled-it.md)). The main types:

- **`Policy`** answers a **`Request`** with a recorded **`Decision`**, through `decide`. It also compiles **`PolicyLayers`** into a **`CandidateSet`**, tests it, and publishes it.
- **`ApprovalSurface`** is the one handle that can record a person's approval or decline, as an **`Approved`** or a **`Declined`**. It is not an action.
- **`KillSwitch`** stops a scope, and **`LimitCheck`** counts a call against a quota.

## Session keeper interface

The design of [`a2m-auth`](../architecture/crates/a2m-auth.md) gives the interface in full ([decision 0085](../decisions/0085-the-session-keepers-interface-as-building-it-settled-it.md)). The auth providers are in the section above. The main types:

- **`Keeper`** answers a **`LeaseRequest`** with a **`SessionLease`**, takes a driver's report of an **`ExpirySign`**, signs in, gives a session's status, and revokes. Nothing else calls an **`AuthProvider`**.
- **`ProviderSet`** holds the providers handed to the keeper, by name.
- **`KeeperWork`** is how a probe, a renewal, or a sign-in runs on a session's home runner. **`InProcessWork`** is the same for one process.
- **`CredentialSink`** is what `inject` places credentials into. No method of it returns a value.

## Source reader interface

The design of [`a2m-sources`](../architecture/crates/a2m-sources.md) gives the interface in full ([decision 0087](../decisions/0087-the-source-readers-interface-as-building-it-settled-it.md)). The main types:

- **`SourceReader`** is the contract of a reader, with three calls: `name`; `check`, which refuses content that is not the reader's format; and `parse`, which gives the whole input as a **`Parsed`**. A reader is given a **`SourceInput`**, which is bytes and never a path.
- **`Sources`** holds the registered readers. `CsvReader`, `JsonReader`, and `XlsxReader` are built in.
- **`LoadedSource`** is a source definition loaded from a compiled target. With it, `Sources` turns an input into **`ValidatedRecords`**, or refuses it as a **`Rejected`** that lists every **`Problem`**.

## Scheduler interface

The design of [`a2m-sched`](../architecture/crates/a2m-sched.md) gives the interface in full ([decision 0093](../decisions/0093-the-schedulers-interface-as-building-it-settled-it.md)). The main types:

- **`Scheduler`** admits a **`Submission`** and queues its job, hands a runner a **`JobLease`**, grants exclusive resources by **`ResourceKey`** and rate budgets by **`BudgetKey`**, and keeps holds by **`HoldScope`**.
- **`Scheduler::resume`** finds a run's one job and acts in the same transaction that would queue its own, and answers `Resumed::Queued` or `Resumed::Reconciling`; the second says how it is held and, for a door, is `busy`, `run_reconciling` through `Reconciling::failure()` ([decision 0201](../decisions/0201-a-resume-is-never-lost-the-browser-takes-no-proxy-and-no-product-build-speaks-a-stands-protocol.md)).
- **`LockTicket`** is taken before the ledger is asked for a record lock, so that a wait cannot miss a release.
- **`SchedSettings`** holds the limits and their defaults. Nothing in the crate sleeps: `sweep` ends every wait whose moment has come.

## Engine interface

The design of [`a2m-engine`](../architecture/crates/a2m-engine.md) gives the interface in full. The main types:

- **`Engine`**, made from **`EngineDeps`**, carries out a job through `run_job`, and a discovery through `discover`, for the runner that holds the job's lease.
- **`JobSpec`** names the compiled target, the tool, the input, and the job's **`JobKind`**: a preview, an execute, a try, a reconcile, a check after a loss, a canary, or a discovery. The job ends as a **`JobOutcome`**.
- **`Control`** is everything the engine asks of the kernel around it: the compiled target, the guard's decision, a session lease, the preview, the approval, locks, intents, and the signed trace. In one process it is the action registry's `InProcessControl`. Two of its calls have defaults: `other_records` answers what other records of an operation recorded, as a value that may have landed, and `step_observed` tells the registry what a reading step observed, which `fixture.record` holds in memory for its trial alone ([decision 0196](../decisions/0196-the-engine-and-the-registry-after-the-acknowledgement-and-the-first-hour.md)).

What [decision 0263](../decisions/0263-a-person-hands-the-scout-files-of-the-projects-sources-folder-by-names-the-discoverers-declare.md) added to the engine:

- **`Control::discovery_files`** gives a discovery job the files the call that started it handed its discoverers: each declared name, to the path the call gave under the project's `sources` folder. The engine checks each path again under a read-only root of that folder before any discoverer runs, and adds only the checked files to `DiscoveryScope::paths`. By default it gives none.

What [decision 0251](../decisions/0251-the-evidence-for-promoting-a-shadow-binding-says-what-it-covers-and-a-shadow-costs-what-it-says.md) added to the engine:

- **`Control::shadow_rate`** asks the control plane for a rate budget for a binding in shadow, all at once or nothing, without waiting, and leaving the share a caller's reads need; by default it grants nothing.
- **`shadow_evidence_environments`** gives the environments a compiled target's shadow evidence must come from, and **`verified_binding`** the verified binding of an operation that acts beside one in shadow.
- **`Engine::shadow_comparisons_lost`** counts the comparisons that were not kept within the shadow budget.

## Action registry interface

The design of [`a2m-actions`](../architecture/crates/a2m-actions.md) gives the interface in full ([decision 0094](../decisions/0094-the-action-layers-interface-as-building-it-settled-it.md)). The main types:

- **`ActionRegistry`** holds every action of the catalog as data, built or not.
- **`Kernel`** is the handle every front door holds. Its `dispatch` takes a **`Request`** and gives a **`Response`**: it checks the input, the health state, and the person need, records one policy decision, runs the handler, and redacts the result.
- **`PersonSurface`** is how the kernel reaches a person. **`ApprovalDesk`** is where a person approves or declines a diff, and where an approval page reads what the diff would change (**`ApprovalPreview`**); no action reaches it. A decline ends the run as `expired`.
- **`Kernel::events`** and **`Kernel::next_events`** give a page of **`RunEvents`** after a sequence number, the second after waiting for one, to the caller that started the run ([decision 0132](../decisions/0132-the-action-registrys-second-pass-a-decline-ends-the-run-and-a-wait-reads-the-schedulers-own-table.md)). **`Kernel::next_events_from`** waits from a **`Followed`**, what the follower last saw, and answers at once when the run came to rest or began again since ([decision 0159](../decisions/0159-a-lost-answer-is-a-write-in-doubt-at-the-doors-and-in-the-clients.md)).
- The faces are data generated from the registry: `command_faces`, `mcp_tools`, `rest_routes`, `sdk_methods`, and **`McpResultFace`**. An action has an SDK method, and its routes say `x-sdk: true` in the API description, when `has_sdk_method` holds: it has an SDK face and a handler, so an action that answers `action_not_built` has none until it is built ([decision 0437](../decisions/0437-what-a-release-publishes-is-kept-and-every-later-build-is-checked-against-it.md), point 5).

The settings schema and loader of `a2m-actions` carry what [decision 0241](../decisions/0241-protected-settings-loosen-only-from-an-administrators-file-and-every-key-says-whether-it-is-built.md) added:

- **`settings::Built`** says for each key whether code reads it (`Yes`), or it is not built (`No`, with what happens `instead` and what it `arrives_with`); a value for a key that is not built is refused by name. **`Entry::built`** carries it into `config.show`, and the setting's `effective` is only `NextStart`: `Effective::AtOnce` no longer exists.
- **`settings::InstallationKeeper`** says who may change the installation's settings file: `Administrator`, `Open` (with the path and why), or `NotChecked`. **`Sources::installation_keeper`** carries it into the loader; only a file kept by an administrator may loosen a protected setting.
- **`Settings::project_file`** gives the project file whose values are layer 6, and **`Settings::project_values`** what it wrote. The warning `project_settings_differ` names the keys where a request's project folder holds a project file that differs from the one the engine applies.
- **`keep_folder_private`**, in `a2m-local`, makes a folder its user's alone; the engine applies it to its data, configuration, and cache folders when it opens.

## REST router

The design of [`a2m-api`](../architecture/crates/a2m-api.md) gives the interface in full ([decision 0116](../decisions/0116-the-rest-router-takes-its-caller-from-the-mount-and-chooses-its-status-in-one-table.md)). The main items:

- **`router`** builds the routes from a `Kernel` and **`ApiSettings`**; every route calls the same `dispatch`.
- **`status_of`** is the one table from a response to an HTTP status. **`openapi`** generates the OpenAPI 3.1 document, and **`served_routes`** lists what the router serves.

## Data model

This is the data model as designed. The tables that exist are those the migrations in `crates/a2m-ledger/migrations/` create, which the [ledger schema](ledger-schema.md) describes column by column; a table named here and not there is not built. The migrations are SQLite's; Postgres is package 6.1 and is not built.

| Table | Holds | Key columns |
| --- | --- | --- |
| `workspaces` | Isolation boundaries | id, name |
| `targets`, `target_versions` | Targets and their immutable published versions | target, version, hash, signature, compiled target, status |
| `bindings_health` | Health of each binding | operation, mode, status, last check |
| `shadow_comparisons`, `shadow_promotions` | How a binding in shadow compared, and its promotion by a person ([decision 0207](../decisions/0207-a-shadow-binding-reads-beside-the-verified-one-and-a-person-promotes-it-on-a-run-of-exact-matches.md)) | operation, binding digest, outcome, differing outputs by name; who promoted it, on how many matches |
| `toolsets`, `toolset_items` | Named tool selections | toolset, version, operation or pipeline, identity mode, approval policy |
| `principals`, `grants` | Callers and what they may do | principal, scope pattern, max effect, role |
| `policy_versions`, `decisions` | Published policy sets; every decision with the rules that made it | version, hash, tests passed; request, answer, determining rules |
| `sessions` | Signed-in states | identity, target, runner affinity, expiry, vault reference |
| `runs` | One row per run | run id, caller, tool version, input hash, state, mode, timings |
| `run_steps`, `run_records` | Pipeline steps and the ledger, one row per commit point | run id, record key, state, intended and observed values, attempts, evidence reference |
| `run_events` | Append-only history of every run | run id, sequence, lease number, kind |
| `record_locks` | Which run may write a record | target, environment, record key, holder, held or quarantined |
| `blobs`, `outbox` | Evidence stored by content hash; messages owed to the outside | hash, kind, retention; destination, attempts |
| `approvals` | Approval records | run id, diff hash, approver, channel, expiry, used flag |
| `jobs`, `runners` | Scheduling | job, lease, runner, capabilities, heartbeat |
| `audit` | Append-only, hash-chained log | sequence, previous hash, actor, action, subject, hash |

## Runner protocol

Runners open every connection themselves, over mutually authenticated HTTPS; `lease` is the reply to a long-polled `claim`. The full design, including leases, lost-lease handling, and secrets, is in spec section 223.

| Message | Direction | Purpose |
| --- | --- | --- |
| `hello` | Runner to control plane | Version, operating system, modes, targets, identities it can serve |
| `lease` | Control plane to runner | A job, with a deadline; the runner must accept or decline |
| `event` | Runner to control plane | Step started, intent recorded, acted, verified, failed |
| `need` | Runner to control plane | A session secret, an approval check, or a rate-budget grant |
| `heartbeat` | Both | Liveness; a missed heartbeat ends the lease |
| `result` | Runner to control plane | Final outcome and evidence references |
| `cancel`, `drain` | Control plane to runner | Stop a job; stop taking jobs |

Delivery is at-least-once. A job whose lease ends without a result is treated as `possible_commit` and reconciled before it is given to another runner.

## Failure codes

Every failure carries a class, a stable code, a plain sentence, and a next step. The classes are closed: the twelve of `FailureClass`. A code is a checked name that belongs to exactly one class; a crate or a pack brings its own codes, and a code never changes, and never moves to another class, once a release has carried it. A release may add codes, so a client branches on the class and reads the code as detail. The classes, the run states, the effect classes, and the evidence levels are closed, and a new member of one is a break; each release keeps its codes, and a later build that changes one fails a check ([decision 0437](../decisions/0437-what-a-release-publishes-is-kept-and-every-later-build-is-checked-against-it.md); package YL4). `tools/contracts.py check` compares the tree with every folder in `contracts/` and names each break: a code removed or renamed, moved to another class, or changed in whether it passes by itself; and likewise for the API description's built actions, the vocabularies, and the settings keys. The settings kept are those that are built: the kernel's schema less each key marked not built, and every pack's settings, read from the manifest each pack's source embeds with `include_str!`, wherever in its crate that file is ([decision 0455](../decisions/0455-a-release-keeps-only-the-settings-it-built-and-the-failure-codes-something-raises.md)). A break passes only where `contracts/next-breaks`, or the folder of a later release, lists it with its record. Some codes are shared on purpose: the same code, with the same class, is declared by every crate that can produce it (for instance `connection_lost`, `timeout`, and `approval_stale`). A code declared with two classes is a fault, and the generator below refuses it. Each `failure_codes!` table also writes a module `row`, a `CodeEntry` per code; a failure of a code is made from its row, `codes::row::NAME.failure(message, next)`, and a step's `FailsAs` from a row, so the class always comes from the table ([decision 0289](../decisions/0289-a-failure-is-made-from-its-codes-row.md)).

The table is generated by `tools/failure_codes.py` from each crate's `failure_codes!` table, the one form every crate uses ([decision 0258](../decisions/0258-every-crate-declares-its-failure-codes-in-one-table.md)), and from the providers' manifests (`failure_codes`), which also name the codes of other crates a pack passes on. A code declared anywhere else, as a literal `FailureCode::fixed` in a crate's source, is a fault of the generator. So is a code declared and raised by nothing ([decision 0455](../decisions/0455-a-release-keeps-only-the-settings-it-built-and-the-failure-codes-something-raises.md)): `--check` names each code whose string no crate's source names, whose constant neither its own crate nor a crate that names its crate (as a `use` of it does) names, and that no other crate's manifest passes on, all read outside the `failure_codes!` tables, outside tests (test folders and files, and each item marked `#[cfg(test)]`), and with comments taken out. A pack's own manifest listing its own code declares it and does not raise it. A code is declared when the part that raises it is built, and not before. The table must not be edited by hand: change the code, then run `python3 tools/failure_codes.py`. The documentation audit fails when the table and the crates differ. "What it says" is the meaning a `failure_codes!` row gives the code; where several crates declare one code, the first crate listed supplies it, and its row is worded to be true for every crate listed: `a2m-core`, then `a2m-provider`, then the other kernel crates by their layer in the table above, lowest first, then the packs and pack bases alike; by name within each. "What to do" is the same row's general next step, and "Passes by itself" says whether the same call may pass when made again unchanged.

<!-- failure-codes:begin (generated by tools/failure_codes.py; do not edit by hand) -->
| Class | Code | Declared in | What it says | What to do | Passes by itself |
| --- | --- | --- | --- | --- | --- |
| `policy` | `ack_not_owned` | `a2m-pack-stream` | An acknowledgement was asked for on a queue the runtime does not own. | Look at the queue with `list`; declare `ownership: runtime_owned` only when nothing else reads it. | no |
| `policy` | `address_looks_like_an_action` | `a2m-pack-web-browser`, `a2m-pack-web-probe` | A watching tab was to be opened on an address that looks like an action. The same name, with the same class, as the web probe's code for an address discovery will not visit. | Open it on the surface's own address; the engine never sends a tab to an address that looks like an action. | no |
| `policy` | `address_unregistered` | `a2m-actions` | A publish would reach an address no person registered for the target, and it was not carried out from the publish page that names it. | Ask for the publish on its page, with `publish.request`, read the new address it names, and press there. | no |
| `policy` | `application_not_owned` | `a2m-pack-desktop` | The program the surface states runs as another user, or on Windows in another session. Windows and macOS; the pack reads none of it. | Start the application in your own session, as yourself, then run the tool again. | no |
| `policy` | `approval_missing` | `a2m-ledger`, `a2m-policy` | A write reached the write phase with no usable approval. | Have a person approve the run's diff, then resume the run. | no |
| `policy` | `approval_refused` | `a2m-policy` | The person or the caller may not approve this run. | Have a person who may approve this run approve it on the approval surface. | no |
| `policy` | `approval_stale` | `a2m-ledger`, `a2m-policy`, `a2m-engine` | The approval is expired, used, declined, revoked, or for another diff. | Preview the run again and have a person approve the new diff. | no |
| `policy` | `caller_missing` | `a2m-api`, `a2m-mcp` | A request came with nothing that names its caller: whoever mounts the router did not put one on it. | Call through the local channel, or with a credential the engine accepts. | no |
| `policy` | `caller_not_verified` | `a2m-actions` | The caller's credential is not one this engine accepts. | Call from the account that runs the engine. | no |
| `policy` | `change_not_shown_whole` | `a2m-local` | A change with more records than its page can show was pressed to approve, and a change not shown in full is not approved on the page. | Decline it from the page, and call the tool again with fewer records in one run. | no |
| `policy` | `compiled_target_unverified` | `a2m-engine` | The compiled target's bytes do not give the hash the job names. | Publish the target again from its stored sources. | no |
| `policy` | `connection_leads_elsewhere` | `a2m-pack-data` | A connection, or the server it reaches, does not lead where the compiled target's surface names: another address, port, database, schema, `search_path`, link mode, or role. It is closed and never used, and nothing is sent. | Check the surface's address, port, database, and schema, and what else may change a connection's settings; then start the run again. | no |
| `policy` | `decompiler_not_pinned` | `a2m-provider` | The program named as the decompiling engine has a digest the product does not pin, or it changed after it was looked at. It is not started. | Install a release of the engine the product pins, or use a product version that pins the one you have. | no |
| `policy` | `denied_by_store` | `a2m-pack-data` | The store refused a write it holds no right to make. | Ask the store's owner for the right to write, or use a route that has it. | no |
| `policy` | `destination_not_declared` | `a2m-auth` | A sign-in would send a credential, a token, or the person to an address the target does not declare for its surfaces in the session's environment. Nothing was sent. | Declare the address as a surface of the target, or correct the setting in `auth.yaml`, and publish the target again. | no |
| `policy` | `device_serial_not_declared` | `a2m-pack-device` | The surface declares no serial for the device of the run's environment, so nothing is written to it. Nothing was sent. | Declare the unit's serial for this environment under `serials:` on the surface, after proving the map against that unit. | no |
| `policy` | `dictionary_include_refused` | `a2m-pack-desktop` | A scripting dictionary a person handed the scout includes a file outside the application's bundle, which the pack never reads. The dictionary is refused whole. | Hand a dictionary that includes nothing outside its bundle, or leave it out of the discovery. | no |
| `policy` | `discovery_identifies_only` | `a2m-pack-device` | A frame other than identification was asked for in the discovery posture. Nothing was sent. | Read a register through an operation, in a run that is not discovery. | no |
| `policy` | `effect_not_declared` | `a2m-provider` | A registration, a start, or a root beyond what the manifest declares. | Declare it in the provider's manifest, where a person can review it, or keep to what the manifest declares. | no |
| `policy` | `effect_too_high` | `a2m-policy` | A grant exists, with a lower highest effect class. | Ask an administrator to raise the grant's highest effect class. | no |
| `policy` | `engine_variable_refused` | `a2m-provider` | A launch names a variable that belongs to the engine. | Remove the variable from the launch; a program is given only variables of its own. | no |
| `policy` | `file_not_on_this_machine` | `a2m-provider`, `a2m-pack-file` | A file's bytes are not on this machine: a sync program keeps them elsewhere, and reading or replacing the file would make it fetch them. Nothing was read or written. | In the sync program's own settings, make the file kept on this machine, then try again. | no |
| `policy` | `file_protected` | `a2m-pack-file` | The file is encrypted, signed, or marked read-only. | Point the document at a file that is not protected, or remove the mark if the file is meant to be changed. | no |
| `policy` | `file_too_large` | `a2m-pack-file` | The file is larger than the pack's limits allow. | Raise the limit the message names, or read or write less. | no |
| `policy` | `folder_not_empty` | `a2m-pack-file` | A folder to remove is not empty. | Remove its entries first, each as its own write. | no |
| `policy` | `frame_budget_exceeded` | `a2m-pack-device` | The run reached the most frames it may send on a link, or a write would leave too few for its read-back. Nothing was sent. | Do the work in smaller runs, or raise the device pack's setting `max_frames_per_run`. | no |
| `policy` | `identity_is_administrator` | `a2m-pack-data` | The identity the lease signs in as is a superuser, bypasses row security, may replicate or make roles, or owns tables of the schema. | Sign in as a role that owns nothing and holds only the rights to read and write the rows the target names. | no |
| `policy` | `identity_not_allowed` | `a2m-auth`, `a2m-policy` | The run does not act for the person the session belongs to. | Call as the person the session belongs to, or use a session of your own. | no |
| `policy` | `intent_missing` | `a2m-provider` | A step that commits was given no intent token, or the wrong one. | Store the record's intent in the ledger first, and pass the token it gives for this record. | no |
| `policy` | `intent_token_missing` | `a2m-engine` | An intent was stored and no token came back for it. | Report this as a fault in the engine; nothing was sent. | no |
| `policy` | `kill_switch_on` | `a2m-policy` | A kill switch covers the request. | Ask an administrator why the kill switch is set, and to release it. | no |
| `policy` | `launcher_not_in_table` | `a2m-pack-mcp` | A surface names an entry file, and its program is not a launcher of the provider's closed table of interpreter forms, or is not given that launcher's one form. Nothing is started (decision 0371). | Name a launcher of the table as `program`, with the fixed arguments of its form as `launcher_args`, or name a program that is itself the server and no entry file. | no |
| `policy` | `lease_superseded` | `a2m-sched` | A call carries a lease that is not the job's current one, or one that has run out. | Stop work on this job; it is recovered under a new lease. | no |
| `policy` | `limit_exceeded` | `a2m-policy`, `a2m-pack-command` | A call is over the records allowed in one run, or over a quota. | Send fewer records in one run, or wait for the next period of the quota. | no |
| `policy` | `link_not_encrypted` | `a2m-pack-data`, `a2m-pack-http`, `a2m-pack-mcp` | The server store is not on this machine, and this build makes no encrypted link to one: no unencrypted link is ever made. | Reach the store at `127.0.0.1` or `::1`, written as an address and not a name. | no |
| `policy` | `no_binding_may_act` | `a2m-engine` | Every binding of the operation the version holds is of a status that may not act here, so no call of it on this version can run; the run fails and is not resumed. | Promote a binding of the operation once its offline test passes, publish the target again, and call the tool again. | no |
| `policy` | `not_granted` | `a2m-policy`, `a2m-pack-mcp` | No permit applies to the caller, the tool, or the action. | Ask an administrator for a grant that covers the caller, the tool, and the action. | no |
| `policy` | `offline_writes_nothing` | `a2m-actions` | An offline test asked for something that would store, lease, or clear a write; its control plane holds nothing and refuses (decision 0177). | Keep the offline test to its own tool against its fixture; a write is proven against a test stand. | no |
| `policy` | `origin_not_declared` | `a2m-pack-web-ui` | The page, a frame of it, or a worker sent a request to an origin that is neither the surface's nor one the operation declares. The browser failed it before it left (decision 0292). | Declare the origin in the operation's `permissions.destinations`, with its scheme, if the system's pages really load from it; otherwise look at why the page sends there. | no |
| `policy` | `package_runner_refused` | `a2m-pack-mcp` | The local server program is a package runner, which fetches the code it runs at the moment it starts; nothing the product starts fetches code then (decision 0299). Nothing is started. | Install the server's package, then name the launcher of the provider's table as the surface's `program` and the installed entry file as `entry`, each with its digest. | no |
| `policy` | `page_write_refused` | `a2m-pack-web-ui` | The page sent a request that could change the system while a read ran, and the tab, which cannot write, failed it (decision 0195). | Look at what the page does when it loads. When a page request asserts the request with `read_asserted: true`, run the tool from a published version, live or in an environment of kind `test` (decision 0394); a page that writes by itself is not read by the web pack, so read the record another way. | no |
| `policy` | `password_read_refused` | `a2m-pack-desktop`, `a2m-pack-web-browser`, `a2m-pack-web-ui` | A step would read the value of a control that holds a secret. | Read another field. A secret is never an output of a tool. | no |
| `policy` | `password_set_refused` | `a2m-pack-desktop` | A step would set a control that holds a secret. Sign-in to a desktop application is not built. | Leave the field out. Signing in to a desktop application is not built in this version. | no |
| `policy` | `peer_not_allowed` | `a2m-local` | A connection from another user, or an engine owned by another user. | Check who owns the engine's runtime directory, remove what is not yours to trust, and start the engine again. | no |
| `policy` | `person_required` | `a2m-actions` | A person's act was asked for by a caller that cannot give one. | Ask the person to do it themselves, through a program that acts for them. | no |
| `policy` | `policy_evaluation_failed` | `a2m-policy` | An error while deciding, or no valid policy set is loaded, so the answer is no. | Check the engine's status; if no valid policy set is loaded, publish one. | no |
| `policy` | `presence_failed` | `a2m-local` | The person dismissed or failed the presence prompt, or this machine has none. | Run the command again and answer the prompt yourself; where this machine has no presence prompt, make the change in the installation's settings file. | no |
| `policy` | `presence_required` | `a2m-policy` | A change to a protected setting that only a person, present, may make. | Have the person make the change and pass the user-presence prompt. | no |
| `policy` | `product_header_refused` | `a2m-pack-http`, `a2m-pack-mcp` | The session's provider placed a header whose name begins `a2m-`, one of the product's own carriers between its parts, which never goes on the wire. Nothing is sent (decision 0270). | Sign in to this surface with a provider made for an HTTP service, such as `api_key`, `bearer_static`, or `http_basic`. | no |
| `policy` | `program_path_unsafe` | `a2m-pack-command`, `a2m-pack-mcp` | The path is not absolute, is a script, is a shell, or the file or a folder above it can be written by another user. | Name the program by its absolute path, and give the file and every folder above it to the engine's user or the administrator, with no write access for anyone else. | no |
| `policy` | `read_only_posture` | `a2m-provider` | A step that commits was asked for where nothing may be written. | Run the operation live, or against a declared test record. | no |
| `policy` | `read_statement_wrote` | `a2m-pack-data` | A statement declared a read tried to change data. | Correct the statement or declare it `effect: write`; do not use the binding as a read. | no |
| `policy` | `record_limit_exceeded` | `a2m-engine` | The run would touch more records than its limit allows. | Split the source, or raise the pipeline's limit and publish again. | no |
| `policy` | `remote_namespace_refused` | `a2m-pack-desktop` | The surface's management namespace names another machine (it begins with two separators). The driver asks only this machine's management service, and asks nothing of it. | Leave `management_namespace` out, or name a namespace of this machine. | no |
| `policy` | `request_not_from_page` | `a2m-local` | A request to the loopback listener did not come from the page this engine served: it named another host or origin, or a press came without the one-time value of the page as it was last shown, or with one already spent. | Open the approval link in your own browser on this machine and press there; a program cannot approve. | no |
| `policy` | `runner_not_enrolled` | `a2m-sched` | A runner that never said `hello` asked for work. | Say `hello` first, then ask for work. | no |
| `policy` | `sandbox_read_only` | `a2m-engine` | A write was asked for where nothing may be written. | Declare the record under the environment's test records, or try a read. | no |
| `policy` | `scan_inconclusive` | `a2m-pack-stream` | A look cannot say whether the record's message is in the log: the stream may lose a message from inside its range, or the range read was not whole. | Look at the log by hand before anything is sent again: the record stays in doubt. | no |
| `policy` | `scan_limit` | `a2m-pack-stream` | A log or a queue holds more than a scan may read, so the scan cannot say whether the record's message is there. | Look at the log or the queue by hand before anything is sent again, or raise the channel's scan limit. | no |
| `policy` | `scout_off` | `a2m-actions` | The setting `authoring.scout.run` is `off`, and `scout.run` was called. | Run the stages one by one (`target.probe`, `discover.run`, `draft.write`), or ask a person to change the setting. | no |
| `policy` | `second_admin_required` | `a2m-policy` | A policy set that needs a second administrator to publish it was offered without one, or the second one may not review it. | Have a second administrator, a person other than the publisher and the author, review and publish it with you. | no |
| `policy` | `setting_locked` | `a2m-actions` | A higher layer tried to change a key the installation file locks. | Remove the key from this layer, or ask whoever installed the engine to change it. | no |
| `policy` | `setting_not_in_force` | `a2m-actions` | A protected setting is set to a value this build cannot put in force. | Take the setting out of the layer the message names, or set it to the value in force, then start the engine again. | no |
| `policy` | `setting_protected` | `a2m-actions` | A layer that may only tighten tried to loosen a protected setting, or a layer set a key it may not set. | Remove the key from this layer, or set it in a layer that may loosen it. | no |
| `policy` | `shadow_not_counted` | `a2m-ledger` | A comparison is not counted: the store is an offline test's, or the run it was made beside is not a live read under way. | Compare a shadow only beside a live read under way. | no |
| `policy` | `shadow_not_promotable` | `a2m-ledger` | A person asked to promote a binding the evidence does not let them promote. | See `anything2mcp shadow report` for how the binding compared, and promote it when it has earned it. | no |
| `policy` | `statement_not_allowed` | `a2m-pack-data` | A statement is not one plain read or one keyed write: it attaches a file, changes a setting or the schema, ends a transaction, is several statements, or is a read that returns no column. | Correct the statement in the statements file: a read is one `SELECT`, and a write is written by the pack from its record. | no |
| `policy` | `surface_not_declared` | `a2m-engine`, `a2m-pack-data`, `a2m-pack-device`, `a2m-pack-http`, `a2m-pack-stream`, `a2m-pack-web-browser`, `a2m-pack-web-probe`, `a2m-pack-web-ui` | An address outside what the target declares was asked for: a surface it does not declare, or an input of the type `url` that is not a destination its operation declares. | Give an address the target declares, or declare it as a surface in `target.yaml` or as a destination in the operation's `permissions`, and publish again. | no |
| `policy` | `take_too_large` | `a2m-pack-stream` | A message taken is larger than may be recorded before it is acknowledged; it was given back. | Raise `limits.message_bytes` in the channel file, or have the message removed from the queue by hand. | no |
| `policy` | `transport_refused` | `a2m-pack-stream` | The connection would carry the session's credential in plain text to an address that is not this machine's own, the broker asks for TLS, or the address is not one a connection can be made to. | A credential goes over plain TCP only to this machine's loopback IP address (127.0.0.1 or [::1]): write the broker's address as one, reach a broker that asks for no sign-in, or use a build that speaks TLS to it. | no |
| `policy` | `update_precondition_unmet` | `a2m-pack-device` | A condition the update declares, or the update mechanism's own state, does not hold on the device. Nothing was written. | Bring the device to the state its documentation sets for an update, such as a pump stopped and no update in progress, then run the operation again. | no |
| `policy` | `variable_refused` | `a2m-provider` | A launch names a variable that makes a program load code other than its own. | Remove the variable from the target's files: a program is never told through its environment to load other code. | no |
| `policy` | `vault_passphrase_not_enabled` | `a2m-vault` | The passphrase route was asked for and is not switched on. | Use the operating system's key store or a key service, or have a person choose the passphrase route in the installation's settings file. | no |
| `policy` | `watch_ended` | `a2m-ledger` | The watch has ended. | Ask for a new watch. | no |
| `policy` | `watch_needs_confirmation` | `a2m-ledger` | A paused watch cannot go on without a person confirming a new one: its version changed or its read drifted. | End this watch and ask for a new one, for its person to confirm. | no |
| `policy` | `watch_not_a_read` | `a2m-ledger` | A watch was asked for an operation that is not a read. | Watch an operation whose effect class is `read`. | no |
| `policy` | `watch_not_confirmed` | `a2m-ledger` | A check was asked of a watch its person has not confirmed. | Ask the person to confirm the watch on their own page. | no |
| `policy` | `watch_not_yours` | `a2m-ledger` | Someone other than the person a watch acts for tried to confirm it. | Ask the person the watch acts for to confirm it on their own page. | no |
| `policy` | `watch_run_not_its_own` | `a2m-ledger` | A run that is not the watch's own read was offered as its check. | End the check with the watch's own run and that run's own result. | no |
| `input` | `action_not_built` | `a2m-actions` | The action is in the catalog and its handler is not built yet. | Call `action.search` to see which actions are built. | no |
| `input` | `action_registry_invalid` | `a2m-actions` | The registry of actions cannot be built: a duplicate name, or a provider's action outside its name space. | Give each action one name, starting with its provider's own name, then start again. | no |
| `input` | `address_invalid` | `a2m-pack-web-browser`, `a2m-pack-web-ui` | An address is not one a tab may be sent to. | Correct the address, or the entry's path and names, in the target's files. | no |
| `input` | `address_value_refused` | `a2m-pack-web-browser`, `a2m-pack-web-ui` | A value cannot stand in one segment of a path: it is refused, never encoded. | Give a value that is one segment of an address: letters, digits, `-`, `.`, `_`, and `~`. | no |
| `input` | `argument_unsafe` | `a2m-pack-command` | A value holds a NUL character, or would become an option. | Give a value that is plain data for this parameter. | no |
| `input` | `arguments_invalid` | `a2m-pack-mcp` | The arguments of a call do not fit the tool's pinned input schema. | Correct the binding's `args`, or the input, so that they fit the tool's input schema. | no |
| `input` | `auth_provider_duplicate` | `a2m-auth` | Two providers were given the same name. | Enable only one provider of that name. | no |
| `input` | `auth_provider_unknown` | `a2m-auth` | An auth entry names a provider that nobody registered. | Name a registered auth provider in `auth.yaml`, or enable the provider that supplies it. | no |
| `input` | `auth_scheme_not_built` | `a2m-pack-http` | The session's provider needs a scheme this pack does not carry out yet. | Use a provider that places a header or a cookie, such as `api_key`, `bearer_static`, or `http_basic`. | no |
| `input` | `auth_settings_invalid` | `a2m-auth` | An auth entry lacks a setting its provider needs, or holds a wrong one. | Correct the entry in `auth.yaml` and publish the target again. | no |
| `input` | `binary_cyclic` | `a2m-pack-command` | A file handed to the scout as a binary leads back to a part of itself that is being read: a resource table below itself, a slice of a fat file that is the fat header or another fat file. | Do not use this file: a program that is whole never leads back to a part it is being read from. | no |
| `input` | `binary_malformed` | `a2m-pack-command` | A file handed to the scout as a binary has a kind the reader knows and is not well formed in a way that is not a cut: a table with an entry out of line, a tree deeper than its format allows, or more entries than the reader takes. | Hand the program as it was built; a file a tool has changed by hand may no longer be one. | no |
| `input` | `binary_truncated` | `a2m-pack-command` | A file handed to the scout as a binary is shorter than its own headers say: a header, a table, a segment, a section, or a slice of it is cut by the end of the file. | Copy the whole file again; a file the copy cut short cannot be read. | no |
| `input` | `binding_not_promotable` | `a2m-actions` | A binding `binding.promote` may not make verified: not a drafted first binding of its operation, or one whose file cannot be rewritten by rule (decision 0189). | Write the binding with `status: draft`, test it and try it, then call `binding.promote`. | no |
| `input` | `body_timed_out` | `a2m-api` | The body did not arrive within the time allowed. Served as 408. | Send the whole body at once. | no |
| `input` | `body_too_large` | `a2m-api` | The body is larger than the limit, or ended early. Served as 413. | Send a smaller input, whole. | no |
| `input` | `capture_not_offered` | `a2m-actions` | No enabled pack watches a person's use of the surface: no discoverer a person drives suits it, so no capture of it can be asked for. | Scout the target with `scout.run` instead, or enable a pack whose discoverer a person drives. | no |
| `input` | `client_action` | `a2m-actions` | The action is carried out by the client end, which did not do so. | Run it as a command on the machine the engine runs on. | no |
| `input` | `client_config_refused` | `a2m-actions` | `client.connect` cannot set a client's configuration: its profile does not read, names no file for this system, or the client's file does not read as a JSON object (decision 0219). Nothing is written. | Correct the client's profile or its file, as the message names, then run the command again. | no |
| `input` | `command_file_unreadable` | `a2m-pack-command` | A file handed to the scout as a completion file or a manual page cannot be read as one: it is not UTF-8 text, it is compressed in a form other than gzip, its gzip form is damaged, or it is no manual page in the `man` or the `mdoc` macros. | Hand the file as plain UTF-8 text, or a manual page compressed with gzip; uncompress a page compressed any other way first. | no |
| `input` | `compiled_expression_unloadable` | `a2m-expr` | A compiled target's expression names a library this engine does not have, no longer checks as the type it states, or was run without a name it reads. | Compile the target again with this engine. | no |
| `input` | `compiled_target_format_unsupported` | `a2m-core` | A compiled target's format is not one this engine loads. | Compile the target again with this engine, or run the engine the compiled target was compiled for. | no |
| `input` | `compiled_target_hash_mismatch` | `a2m-core` | A compiled target's bytes do not match the hash it was published with. | Compile and publish the target's version again, so that a new compiled target is stored; the tool stays unavailable until then. | no |
| `input` | `compiled_target_unreadable` | `a2m-core` | A compiled target's bytes match its hash and are not a well-formed compiled target. | Compile the target again with this engine and publish the new version. | no |
| `input` | `content_type_unsupported` | `a2m-api` | A body was sent with a type other than JSON. Served as 415. | Send the input as JSON, with the content type application/json. | no |
| `input` | `contract_unsupported` | `a2m-provider` | A manifest was written for a contract this build does not accept. | Use a version of the provider written for this contract. | no |
| `input` | `credential_missing` | `a2m-auth` | A credential reference names nothing that is stored. | Store the credential under that reference, then try again. | no |
| `input` | `credential_not_carried` | `a2m-pack-data`, `a2m-pack-stream` | The session's credential is not a user and a password, which is all a server store is signed in with. | Sign in to the target with a provider that gives a user and a password. | no |
| `input` | `credential_of_another_target` | `a2m-auth` | An auth entry names a credential reference that belongs to another target: its first part is not the target's name. | Name a reference under `vault://<target>/` in `auth.yaml`, and store the credential there. | no |
| `input` | `credential_unreadable` | `a2m-auth` | What is stored under a reference does not open for this target: it was stored for another target, or before a credential was bound to its target. | Store the credential again: type it on the sign-in page when an agent asks, or sign in with `anything2mcp session login`. | no |
| `input` | `decision_not_found` | `a2m-policy` | No decision with that identifier is recorded. | Give the identifier of a recorded decision; it starts with `d_`. | no |
| `input` | `decompiler_spec_invalid` | `a2m-provider` | A decompiling engine is stated with no pinned digest, or without one `{file}` argument. | State the digests of the engine's releases and one `{file}` argument. | no |
| `input` | `description_too_large` | `a2m-pack-http` | A published description that is larger, deeper, or wider once its references are followed than the import reads. | Give a description of the part of the service the tools need, with fewer or smaller schemas. | no |
| `input` | `description_unreadable` | `a2m-pack-http` | A published description that cannot be read. | Give an OpenAPI 3.0 or 3.1 document or a Swagger 2.0 document, as JSON or YAML, that holds its own schemas. | no |
| `input` | `discovery_file_not_taken` | `a2m-actions` | A call names a file for a discovery under a name that no discoverer suiting the target's observed surfaces reads (decision 0263): a file nobody reads is a mistake, never ignored. Nothing starts. | Name each file under one of the names the message lists, or leave it out. | no |
| `input` | `draft_conflict` | `a2m-files` | The drafter would overwrite a file the developer changed. | Keep the file as it is, or remove it and draft again. | no |
| `input` | `expression_cost_exceeded` | `a2m-expr` | Evaluation passed its cost limit. | Give the expression less to work over, such as fewer rows, or simplify it. | no |
| `input` | `expression_invalid` | `a2m-expr` | The text does not parse, or names something the environment does not declare. | Correct the expression at the place the message names, using only names the place declares, and compile again. | no |
| `input` | `expression_type_error` | `a2m-expr` | A type does not fit, including any use of a float. | Give each operator and function values of the types it takes; write a decimal as decimal("1.5"). Then compile again. | no |
| `input` | `file_not_staged` | `a2m-pack-web-ui` | A file input was given a name that is not the name of a file in the staging folder: it holds no file of that name, or the name is not one plain name. | Put the file in the staging folder under the name, and give that name; no path is taken. | no |
| `input` | `file_type_mismatch` | `a2m-sources` | The content is not the format its reader reads. | Give a file of the format the source definition names. | no |
| `input` | `file_unavailable` | `a2m-provider` | A file under a checked path could not be read or written. | Check that the file exists and that the engine may use it, then try again. | no |
| `input` | `finding_invalid` | `a2m-core` | A finding claims more than its source, its parents, or its content allow. | Make the finding at the level its source supports, with the parents it names, and record it again. | no |
| `input` | `fixture_no_scrub_rule` | `a2m-actions` | `fixture.record` met an observed value that no scrubbing rule covers, and wrote nothing (decision 0196). | Write this fixture by hand with synthetic values, or record a read whose values are text, numbers, dates, choices, true or false, or empty. | no |
| `input` | `fixture_not_replayed` | `a2m-actions` | The test `fixture.record` made does not pass against its scrubbed fixture, or an observed value would be in a file; nothing was written. | Write this fixture by hand with synthetic values its field types take. | no |
| `input` | `fixture_read_failed` | `a2m-actions` | The read `fixture.record` runs did not complete in the sandbox, or gave nothing to record; nothing was written. | Correct the draft or the input and record again, or write this fixture by hand. | no |
| `input` | `focus_invalid` | `a2m-pack-web-probe` | The focus of a discovery is not in the form this pack reads. | Give paths of the surface, such as `/orders`. | no |
| `input` | `formula_without_value` | `a2m-sources` | A formula cell in the data has no cached value. | Open the workbook in the program that made it, let it calculate, save it, and give it again. | no |
| `input` | `hello_invalid` | `a2m-local` | A connection's first request was not a well-formed hello. | Reach the engine through `anything2mcp` or its SDK bridge, which say hello first; stop whatever else holds the engine's channel. | no |
| `input` | `idempotency_key_invalid` | `a2m-ledger`, `a2m-api` | An idempotency key that is not 1 to 200 printable characters. | Send a short printable key, the same for every repeat of one call. | no |
| `input` | `idempotency_key_reused` | `a2m-actions` | An idempotency key the caller already gave with another call: another tool, environment, input, or source. | Use a new key for a new call. To get the run of the call the key was first given with, make that call again with the key, or call `run.list` with `idempotency_key`. | no |
| `input` | `image_not_accepted` | `a2m-pack-device` | The firmware image is not one an update will send: it is not hexadecimal text, it is not of the form an image has, its checksum does not hold, it is larger than the device takes, it carries another version than the operation names, or its digest is not on the operation's list. Nothing was sent. | Give the image the device's maker issued for this model, as hexadecimal text, with the version it carries; add its digest to the operation's `accepts` list if the list is meant to include it. | no |
| `input` | `input_invalid` | `a2m-engine` | An input is missing, is not declared, or is not of its type. | Call the tool with every input it declares, each of its type. | no |
| `input` | `job_invalid` | `a2m-engine` | The job names a tool, an environment, or a surface the compiled target does not hold, or a job of a kind the tool cannot run as. | Submit the job again for a tool, environment, and surface the published compiled target holds. | no |
| `input` | `kind_owner_conflict` | `a2m-provider` | Two enabled providers own one kind for one surface kind. | Disable one of the two providers; a kind has one owner for a surface kind. | no |
| `input` | `kind_owner_missing` | `a2m-provider` | A kind a provider reads has no enabled owner. | Enable the provider that owns the kind, as the message names it. | no |
| `input` | `lint_failed` | `a2m-files` | A target's files have an error diagnostic, so they do not compile. | Correct the file at the place the first diagnostic names, as its fix says, and compile again. | no |
| `input` | `local_api_mismatch` | `a2m-local` | The client and the engine speak different versions of the local API. | Install the same version of the client and the engine, then run the command again. | no |
| `input` | `log_settings_invalid` | `a2m-logs` | The settings the logs and traces were started with cannot be used, such as a log file size of zero. | Set the log file size to at least one byte, then start the engine again. | no |
| `input` | `manifest_invalid` | `a2m-provider` | A manifest does not parse, or a field of it is wrong. | Correct the field of the provider's manifest the message names, then load it again. | no |
| `input` | `mcp_not_served` | `a2m-mcp` | The service was asked for a toolset or a revision it does not serve. | Ask for a toolset and a revision this build serves; the message names what was asked for. | no |
| `input` | `message_too_large` | `a2m-pack-stream` | A message is larger than its channel allows. | Shorten the message; nothing was sent. | no |
| `input` | `method_not_allowed` | `a2m-api` | The route exists and does not take this method. Served as 405. | Read /api/v1/openapi.json for the method of each route. | no |
| `input` | `mode_conflict` | `a2m-provider` | A mode or surface kind is registered twice, or with two archetypes. | State each mode and surface kind once, with its one archetype, or disable one of the two providers. | no |
| `input` | `mode_unknown` | `a2m-provider` | Nobody registered the mode. | Enable the provider that brings the mode; `anything2mcp provider list` shows them. | no |
| `input` | `name_case_conflict` | `a2m-pack-file` | A folder holds a name that differs from the one given only in case. | Give the name exactly as the folder holds it, or choose a name that differs in more than case. | no |
| `input` | `network_not_recognizable` | `a2m-actions` | The network the machine is on states too few facts to be recognized again, so no reach can be named on it. | Connect to a network with a default route and a gateway that answers, then name it again. | no |
| `input` | `not_built` | `a2m-pack-mcp` | A part of the pack's design that this build does not carry out. | See the build order in the design of the crate `a2m-pack-mcp` for what is built. | no |
| `input` | `not_found` | `a2m-ledger` | The thing asked for is not there. | Check the identifier, and give one an earlier result gave. | no |
| `input` | `not_made_ready_not_a_read` | `a2m-actions` | `draft.ready` was named a tool that is a write or a pipeline: it makes reads ready alone, and leaves a write's test, fixture, and promote to be done by hand and by a person (decision 0443). | Write the write's offline test and fixture with `draft.write`, run `test.run`, and have a person promote its binding; or leave it out of `tools`. | no |
| `input` | `not_made_ready_untried` | `a2m-actions` | `draft.ready` was named a read that no trial in this project has read a value through: the newest finished scout did not prove it, and the caller has no completed trial of it made since (decision 0443). | Try the read with `draft.try` and an input it answers, or sign in where it waits for a sign-in, then call `draft.ready` again. | no |
| `input` | `not_registered` | `a2m-provider` | Nobody registered the reader, action, or discoverer asked for. | Enable the provider that brings it. | no |
| `input` | `nothing_recorded` | `a2m-pack-stream` | A step that settles a record by what its earlier steps recorded was given nothing recorded, or nothing of the form it needs; nothing was sent. | Settle the record by hand, with what the run's ledger holds for it. | no |
| `input` | `pack_settings_invalid` | `a2m-provider` | A pack's settings group does not fit the schema its manifest declares. | Correct the setting, or remove it to take the default. | no |
| `input` | `page_not_found` | `a2m-local` | No page waits at that address on the loopback listener: it was answered, its window ended, the engine started again, or the address never held one. | Look at the run with `anything2mcp run get`; if it still waits, resume it for a new link. | no |
| `input` | `path_denied` | `a2m-provider` | The engine's user may not write the file, or the file system is read only. | Let the engine's user write the file and its folder, or name another file. | no |
| `input` | `path_field_conflict` | `a2m-api` | A field given in the path was given again, differently, in the body. | Give the field once, in the path. | no |
| `input` | `path_missing` | `a2m-provider` | A file, or the folder that holds it, is not there. | Make the file and its folder exist, or name one that does. | no |
| `input` | `path_not_allowed` | `a2m-provider`, `a2m-files`, `a2m-actions` | A path or a link leaves the folder it must stay inside, or is not a form a path may take. | Name a file inside its root by a relative path with forward slashes and no `..`, or a folder by its absolute path where a setting asks for one. | no |
| `input` | `path_value_refused` | `a2m-pack-http` | A value that cannot stand in one segment of a path. | Give a value that is one plain name or number, as the system shows it. | no |
| `input` | `payload_not_encodable` | `a2m-core` | Data holds something the compiled target's encoding cannot carry, such as a float. | Give the value as a decimal, a text, or a whole number of at most 64 bits, and keys as text. | no |
| `input` | `payload_unreadable` | `a2m-core`, `a2m-pack-data`, `a2m-pack-device`, `a2m-pack-file`, `a2m-pack-mcp`, `a2m-pack-stream` | A payload in a compiled target does not decode as the type its reader expects. | Compile the target again with the pack versions this engine has, and publish the new version. | no |
| `input` | `policy_tests_failed` | `a2m-policy` | A policy set whose own test table does not pass. | Correct the set or its tests until its test table passes, then publish it again. | no |
| `input` | `powershell_file_unreadable` | `a2m-pack-command` | A file handed to the scout as a PowerShell help file or a module manifest cannot be read as one: it is not UTF-8 text, it is not well-formed XML or declares a document type, it passes a bound of the reader, or it is no help file (`helpItems`) or no manifest (one table of data). | Hand the module's XML help file, `<module>-help.xml`, or its manifest, `<module>.psd1`, as it ships; a help file that declares a document type is never read. | no |
| `input` | `program_not_found` | `a2m-provider` | A program to start was not found where it was declared. | Install the program at the absolute path that is declared, or correct the declaration. | no |
| `input` | `project_folder_missing` | `a2m-provider` | The run has no project folder, and a file of the project was asked for. | Call the tool from inside the project, through a door that states its folder. | no |
| `input` | `project_of_another_store` | `a2m-actions` | The project folder a call names is worked from another engine's store: its mark, `.anything2mcp/engine`, names another store than this engine's, so what this engine holds is not that project's scouts, trials, or published versions (decision 0445). The call is refused whole. | Run the command with the engine home the project was worked from (`A2M_HOME`, or none); or, to work the project from this engine from now on, remove `.anything2mcp/engine` in the project folder and run it again. | no |
| `input` | `promotion_unproven` | `a2m-actions` | A promotion whose evidence is missing: an offline test of the operation that is missing or did not pass, or a read no trial proved at `executed` (decision 0189). | Write or correct the operation's offline test, run `test.run`, and promote again. | no |
| `input` | `protocol_unknown` | `a2m-pack-stream` | The surface names a wire protocol this build does not speak. | Use a protocol the stream pack lists, or a build that speaks this one. | no |
| `input` | `provider_ambiguous` | `a2m-provider` | Two ready providers drive one mode and no setting names one. | Name the provider for the mode in the settings, or disable one of the two. | no |
| `input` | `provider_duplicate` | `a2m-provider` | Two providers carry one name. | Remove or disable one of the two. | no |
| `input` | `publish_gates_not_passed` | `a2m-actions` | A publish whose offline tests did not all run and pass: a version is published only when they do (spec section 403, decision 0180). | Call `test.run` with the target to see what each test found, correct the test, its fixture, or the binding, and publish again. | no |
| `input` | `query_invalid` | `a2m-api` | The query of a request is not well formed, names a field twice, or was sent on a route that reads its input from the body. | Give each field once, as name=value, percent-encoded, on a route that reads its input from the query; otherwise send the input as a JSON object in the body. | no |
| `input` | `registration_conflict` | `a2m-files` | Two registrations for the same key, or one that is not the registering pack's to make. | Disable one of the two providers, or correct the provider that registers a name that is not its own. | no |
| `input` | `route_conflict` | `a2m-api` | Two routes the registry describes cannot both be served: the same method and path, or a path field named differently at the same place. Returned when the router is made, never to a caller. | Give each action's alias a path no other route has. | no |
| `input` | `route_not_found` | `a2m-api` | No route has this path. Served as 404. | Read /api/v1/openapi.json for the routes. | no |
| `input` | `schema_invalid` | `a2m-core`, `a2m-pack-command`, `a2m-pack-device`, `a2m-pack-stream` | A file, an input, or a name does not have the shape its schema requires. | Correct the field or the name the message points to, in the shape its schema gives, and try again. | no |
| `input` | `sealed_format_unsupported` | `a2m-vault` | A sealed item's format or cipher is one this engine does not know. | Open it with the engine version that sealed it, or a newer one. | no |
| `input` | `sealed_item_mismatch` | `a2m-vault` | A sealed item does not open under the context given. | Open it with the workspace, owner, kind, and item it was sealed for; if those are right, make the sealed item again. | no |
| `input` | `secret_too_short` | `a2m-vault` | A secret a person chose is shorter than 8 bytes, too short to be masked safely, so it is not stored or registered. | Choose a secret of at least 8 bytes, then store it again. | no |
| `input` | `settings_invalid` | `a2m-actions` | A settings file, variable, or flag cannot be loaded. | Correct the key or value at the place the message names, or remove it, then start again. | no |
| `input` | `socket_path_too_long` | `a2m-local` | The engine's socket would have a path longer than the system takes for a socket's address, so the channel can be neither made nor reached there. | Set `A2M_HOME` to a folder with a shorter full path, then run the command again. | no |
| `input` | `source_definition_invalid` | `a2m-sources` | The compiled source definition does not hold together. | Compile the target again with this engine. | no |
| `input` | `source_encoding_unsupported` | `a2m-sources` | The text is in an encoding the readers do not read. | Save the file as UTF-8 and try again. | no |
| `input` | `source_key_not_unique` | `a2m-sources` | Two rows have the same key. | Correct the rows the problems name so that each key appears once, or key the source on fields that tell its rows apart, and give the input again. | no |
| `input` | `source_malformed` | `a2m-sources` | The content claims its format and breaks the format's rules. | Save the file again from the program that made it, or correct it, and try again. | no |
| `input` | `source_protected` | `a2m-sources` | The workbook is encrypted, so it cannot be read without its password. | Save a copy without the password and give that copy. | no |
| `input` | `source_reader_unknown` | `a2m-sources` | The definition names a reader that is not registered. | Use a reader the engine has, or enable the plug-in that brings it. | no |
| `input` | `source_template_unknown` | `a2m-sources` | The input's structure matches no template version the definition knows. | Give a file made from a known version of the template, or add this layout to the source definition as a new version. | no |
| `input` | `source_too_large` | `a2m-sources` | The bytes, the rows, the cells, or one cell exceed the caller's limits. | Give a smaller input, or have the limits raised by whoever sets them. | no |
| `input` | `step_not_usable` | `a2m-pack-command`, `a2m-pack-desktop`, `a2m-pack-http`, `a2m-pack-web-ui` | A step this driver cannot carry out as the compiled target wrote it: a command that writes on a step that is no commit point, or a step whose program was not started first. | Compile the target again with the version of the command pack that is enabled. | no |
| `input` | `target_unreadable` | `a2m-files` | The target folder could not be read, or a draft could not be written into it. | Make the target folder exist and be readable and writable by this user, then try again. | no |
| `input` | `tls_not_enabled` | `a2m-auth`, `a2m-pack-http`, `a2m-pack-mcp` | This build cannot make HTTPS requests, and an address needs one. | Use a build of the engine with the `tls` feature switched on. | no |
| `input` | `tls_setup_failed` | `a2m-auth`, `a2m-pack-http`, `a2m-pack-mcp` | The TLS configuration of this machine could not be made. | Run `anything2mcp doctor`, and report it if it says nothing more. | no |
| `input` | `trace_holds_secret` | `a2m-logs` | A finished trace still held a registered secret at the check on the way out. The trace is dropped and never reaches the store. | Register every secret with the vault before the run uses it, then run again. | no |
| `input` | `trust_roots_unreadable` | `a2m-auth` | A file of further trust roots holds no certificate, or one that cannot be read. | Name a PEM file that holds the certificate of the authority, and start the engine again. | no |
| `input` | `value_absent` | `a2m-expr` | A value was absent where the expression needs one. | Give the value, or a default with coalesce, and run again. | no |
| `input` | `value_begins_formula` | `a2m-pack-file` | A value for delimited text begins with a character that makes a spreadsheet program run the text as a formula. | Give a value that begins otherwise, or mark the field `may_begin_formula: true` in the document if no spreadsheet program opens the file. | no |
| `input` | `value_not_bindable` | `a2m-pack-data` | An argument is missing, or is a list or a record. | Pass a text, a whole number, a decimal, a date, or a truth value for the parameter. | no |
| `input` | `value_out_of_range` | `a2m-core`, `a2m-expr`, `a2m-pack-device`, `a2m-pack-file`, `a2m-pack-stream` | A decimal, a date, a key part, or a moment cannot be represented. | Give a value inside the range the message names. | no |
| `input` | `value_unparsable` | `a2m-expr`, `a2m-pack-file` | A parse function met text it cannot read. | Correct the value, or the field type's format, and run again. | no |
| `input` | `vault_passphrase_mismatch` | `a2m-vault` | The passphrase given is not the one the keys were wrapped under. | Start again with the passphrase used before. | no |
| `input` | `vault_passphrase_missing` | `a2m-vault` | The passphrase route is chosen and no passphrase was given. | Set the environment variable the message names to the passphrase, then run the command again. | no |
| `input` | `vault_passphrase_too_short` | `a2m-vault` | The passphrase given is too short to derive a root key from. | Start again with a passphrase of at least 16 bytes. | no |
| `input` | `watch_interval_too_short` | `a2m-sched` | A watch's interval is below the floor, or not a whole number of seconds. | Ask for a longer interval, in whole seconds. | no |
| `input` | `watch_too_long` | `a2m-sched` | A watch's end is further away than the longest a watch may last. | Ask for an earlier end. | no |
| `session_expired` | `credential_refused` | `a2m-auth` | A stored credential was refused by the side that checks it, or was tried as often as the auth file allows. | Check the stored credential and store it again if it changed, then sign in with `anything2mcp session login`. | no |
| `session_expired` | `device_required` | `a2m-auth` | The session's credentials are held on a runner that is not this one. | Run on the session's home runner, or sign in again on this one. | no |
| `session_expired` | `headroom_short` | `a2m-auth` | The session will end before the time a pipeline asked to have left. | Sign in again to start a fresh session, then start the pipeline. | no |
| `session_expired` | `lease_stale` | `a2m-auth` | The lease was granted for credentials that have since been replaced. | Ask for a new lease. | no |
| `session_expired` | `needs_login` | `a2m-auth`, `a2m-engine`, `a2m-pack-stream`, `a2m-pack-web-probe` | The session is absent, expired, or revoked, or a wait for it ran out. | Sign in with `anything2mcp session login`, then resume the run, which reconciles any write it began before it writes again. | no |
| `session_expired` | `needs_mfa` | `a2m-auth` | A second factor is demanded and no person answered in time. | Answer the sign-in request, or sign in with `anything2mcp session login`, then run the tool again. | no |
| `edge_blocked` | `challenge_page` | `a2m-pack-web-ui` | The edge answered with a challenge that asks the visitor to prove it is not a program. The engine does not answer a challenge, and the surface is left alone for a while. | Ask the application's owner or its IT to allow the engine's address; a person may open the application in a browser of their own. | no |
| `edge_blocked` | `edge_blocked` | `a2m-engine` | The surface is blocked at the edge. | Wait for the block to end; the engine does not go around it. | no |
| `edge_blocked` | `forbidden_by_edge` | `a2m-pack-http`, `a2m-pack-web-probe`, `a2m-pack-web-ui` | The surface answered with status 403 and a page that is not data, which the endpoint does not declare: a refusal by the edge in front of the system (decision 0357). | Look at the surface in a browser, and ask its owner before trying again. The answer is never retried. | no |
| `edge_blocked` | `image_protected` | `a2m-pack-device` | The firmware image looks encrypted or protected, and is not read further. | Supply the image in the form the maker publishes for inspection, or inspect the device through its own interface; nothing here decrypts an image or recovers a key. | no |
| `edge_blocked` | `net_fact_refused` | `a2m-netstate` | The system refused this fact to this user. | Read the fact in the system's own network settings, as a person who may see it. | no |
| `edge_blocked` | `rate_limited_by_edge` | `a2m-pack-http`, `a2m-pack-web-probe`, `a2m-pack-web-ui` | The surface answered with status 429, which the endpoint does not declare: a limit set by the edge in front of the system (decision 0357). | Look at the surface in a browser, and ask its owner before trying again. The answer is never retried. | no |
| `edge_blocked` | `sign_in_blocked` | `a2m-pack-web-ui` | The sign-in page of a form sign-in shows the block the target declares: a locked account, a check the engine does not get past (decision 0245). | Look at the application in a browser of your own; sign in again once the block is lifted. | no |
| `drift` | `anti_forgery_not_found` | `a2m-pack-http`, `a2m-pack-web-ui` | The page that gives a write's anti-forgery token answered without a page, or holds no token where the endpoint's `anti_forgery` says. The write was not sent (decision 0357). | Look at where the page holds its token now, and correct the endpoint's `anti_forgery`. | no |
| `drift` | `binding_drifted` | `a2m-engine` | Every binding that could carry the call out has drifted. | Repair the binding, verify it, and publish the target again. | no |
| `drift` | `channel_missing` | `a2m-pack-stream` | The channel, or the runtime's own subscription on it, is gone. | Check the channel's `address` and `subscription` against the broker, and correct the channel file. | no |
| `drift` | `description_changed` | `a2m-pack-http` | The published description an `api` binding is pinned to is not, where it was served, the text it was pinned from: it has changed, or is no longer served there. Nothing of the call was sent (decision 0389). | Import the description again, review what changed in the drafts, and publish a new version. | no |
| `drift` | `entry_digest_changed` | `a2m-pack-mcp` | The entry file of a server started through a launcher is not the one its `entry_digest` pins. Nothing is started (decision 0371). | Review the installed server's entry file, write the SHA-256 of its bytes under the surface's `entry_digest`, and publish. | no |
| `drift` | `file_missing` | `a2m-pack-file` | The file is not at its declared path. | Put the file back, or correct `file` in the document. | no |
| `drift` | `file_unreadable` | `a2m-pack-file` | The file does not parse as its declared format. | Correct the file, or correct `format` and `dialect` in the document. | no |
| `drift` | `fingerprint_mismatch` | `a2m-pack-desktop`, `a2m-pack-file`, `a2m-pack-web-ui` | A table's headings are not the ones its screen declares, or a row of it does not have one cell for each heading. | Take a snapshot of the window and correct the screen's `header`. | no |
| `drift` | `key_not_unique` | `a2m-pack-data`, `a2m-pack-file` | A key matched more than one row. | Correct the record's key in the statements file; nothing was written. | no |
| `drift` | `locator_ambiguous` | `a2m-pack-desktop`, `a2m-pack-web-ui` | A locator matched more than one element, or a screen's window matched more than one window. The first is never picked. | Give the screen a locator the application holds once, or leave the field out. | no |
| `drift` | `locator_not_found` | `a2m-pack-desktop`, `a2m-pack-web-ui` | No locator of a field or a table matched. | Take a snapshot of the window and correct the screen's locators. | no |
| `drift` | `output_unparsed` | `a2m-pack-command` | A read's output does not parse as its declared form. | Compare what the program prints now with the command's `output` in `commands/`, and correct the file if the program changed. | no |
| `drift` | `place_not_found` | `a2m-pack-file` | A named place, a header, a table, a name, or a key is not in the file. | Look at what changed in the file, and correct the document or put the place back. | no |
| `drift` | `position_out_of_range` | `a2m-pack-stream` | A position the run was to read from is no longer held. | Start from a position the log still holds. | no |
| `drift` | `program_digest_changed` | `a2m-pack-command` | The file's digest is not one listed for this platform. | Review the program that is installed now, add its SHA-256 under `program.digests` for this platform, and publish. | no |
| `drift` | `program_missing` | `a2m-pack-command` | The program's path is missing, is not a file, or cannot be run. | Install the program at the path its surface declares, or correct the path in the target's files. | no |
| `drift` | `program_version_unsupported` | `a2m-pack-command` | The version is absent, does not match, or is not a supported one. | Install a supported version, or check the target's files against this one and add it to `program.version.supported`. | no |
| `drift` | `read_not_by_key` | `a2m-pack-data` | A read of one record by its key gave a record with another key, or does not return the key. | Compare the record's key and columns in the statements file with the store's catalog, and publish a corrected version. | no |
| `drift` | `reply_changed` | `a2m-files` | A call's reply no longer has the shape the target's files describe. | Explore the call again, update its file to the reply the system now gives, and run the tests again. | no |
| `drift` | `response_schema_mismatch` | `a2m-pack-http`, `a2m-pack-mcp`, `a2m-pack-stream`, `a2m-pack-web-ui` | The body of an accepted answer does not fit the declared shape. | Compare the system's answer with the endpoint's `response_schema` and the operation's outputs, and correct the one that is wrong. | no |
| `drift` | `round_trip_failed` | `a2m-pack-file` | The patch editor would change something it was not asked to change. | Report this with the file's layout; until the editor covers it, the file is read-only. | no |
| `drift` | `schema_changed` | `a2m-pack-data` | A table, column, parameter, or trigger differs from the compiled target. | Run discovery again, compare the store's catalog with the statements file, and publish a corrected version. | no |
| `drift` | `shape_changed` | `a2m-pack-stream` | The channel's shape or key is not the one its file declares. | Run a discovery of the broker and correct the channel file. | no |
| `drift` | `site_tool_changed` | `a2m-pack-web-ui` | The tool the page registered under the name is not the tool the binding was written for: its description or its input schema differs from the digest the binding holds. Nothing was sent. | Look at the tool the page offers now, and, if it is the one meant, pin the digest this message gives in the binding. | no |
| `drift` | `site_tool_not_offered` | `a2m-pack-web-ui` | The page registered no tool of the name the binding calls, in the time given. | Look at which tools the page offers now; the page may have stopped offering the tool, or the binding names another. | no |
| `drift` | `store_file_replaced` | `a2m-pack-data` | The embedded store's file was replaced by another file at its name after the run was prepared. | Check what replaced the file, then start the run again. | no |
| `drift` | `upstream_tool_changed` | `a2m-pack-mcp` | A mapped tool's definition is not the one its mapping pins, or the tool is gone. | Run discovery on the surface, review what changed, and pin the tool's new digest in the mapping. | no |
| `unknown_variant` | `answer_holds_secret` | `a2m-pack-http` | A `bytes` answer whose body holds a registered secret, so neither its length nor its digest is returned (decision 0249); or an answer that quotes the anti-forgery token the write carried, so no value that holds it is returned (decision 0357). | Ask the system's owner why the answer carries the credential, or read it another way. | no |
| `unknown_variant` | `anti_forgery_unreadable` | `a2m-pack-http`, `a2m-pack-web-ui` | The page that gives a write's anti-forgery token holds two different tokens where the endpoint says, or one that cannot be sent. The write was not sent (decision 0357). | Look at the page; a token is read from one place that holds one text. | no |
| `unknown_variant` | `application_ambiguous` | `a2m-pack-desktop` | More than one running application is the program and the name the surface states. | Close the other copies of the application, then run the tool again. | no |
| `unknown_variant` | `application_not_proven` | `a2m-pack-desktop` | The running application's program is not the one whose digest the surface states. | Review the program that is installed, then state its digest in the surface, or reinstall the reviewed one. | no |
| `unknown_variant` | `argument_masked` | `a2m-engine` | An argument of a step holds the mask of a value the redactor removed. | Change the step so that it does not take a value the system shows as a registered secret. | no |
| `unknown_variant` | `binary_kind_unknown` | `a2m-pack-command` | A file handed to the scout as a binary is of no kind the reader has a rule for: it is not an ELF, a PE, or a Mach-O file, or it is a PE file with no PE signature. | Hand a program of one of the three kinds the reader has a rule for: an ELF, a PE, or a Mach-O file. | no |
| `unknown_variant` | `browser_not_owned` | `a2m-pack-web-browser`, `a2m-pack-web-ui` | The program that started is not a browser this crate can own. | Check the browser program named in the settings, and its version. | no |
| `unknown_variant` | `catalog_too_large` | `a2m-pack-data` | The store's catalog holds more tables than one read takes, so what a write reaches cannot be shown. | Declare `hazard: data_elsewhere` on the statement and on its operation, so that a person agrees to each write; or keep the tables in a smaller store. | no |
| `unknown_variant` | `certificate_expired` | `a2m-auth`, `a2m-pack-http`, `a2m-pack-mcp` | The certificate's time has run out. | Ask the system's operator to renew its certificate, and check this machine's clock. | no |
| `unknown_variant` | `certificate_name_mismatch` | `a2m-auth`, `a2m-pack-http`, `a2m-pack-mcp` | The certificate is for another name than the address. | Check the address in the target's files: the certificate names another host. | no |
| `unknown_variant` | `certificate_not_verified` | `a2m-auth`, `a2m-pack-http`, `a2m-pack-mcp` | The certificate was refused for another reason, or the operating system's verifier refused it without saying which. | Check the system's certificate with its operator. | no |
| `unknown_variant` | `certificate_untrusted` | `a2m-auth`, `a2m-pack-http`, `a2m-pack-mcp` | The certificate the system showed does not lead to an authority this machine trusts. | If the system uses an authority of your organization, name that authority's certificate in the setting `network.trust_roots`; otherwise do not connect to it. | no |
| `unknown_variant` | `commit_not_redrawn` | `a2m-pack-web-ui` | A value was committed on a surface whose platform profile says the page sends a change and draws the form again, and the page did not draw it again in the time given. What became of the value is not known. | Look at the page and find out what the application holds; then correct the surface's `profile`, or the screen, before running the tool again. | no |
| `unknown_variant` | `commit_skipped` | `a2m-engine` | A binding ended without sending its write. | Correct the binding, or the step's condition, and publish the target again. | no |
| `unknown_variant` | `condition_not_boolean` | `a2m-engine` | A condition gave a value that is neither true nor false. | Compare the condition in the target files with what the system returns. | no |
| `unknown_variant` | `decompiler_output_unknown` | `a2m-provider` | A decompiling engine printed something that is not in the form the product reads; none of it was used. | Use an engine and an adapter that print one JSON object to a line, as the decompiler's documentation says. | no |
| `unknown_variant` | `device_identity_mismatch` | `a2m-pack-device` | The device's model, firmware version, or serial is not the one the map and the surface name. | Check the surface's address and protocol in this environment; if the unit was replaced, prove the map against the new unit in a test environment and declare its serial. | no |
| `unknown_variant` | `discoverer_cannot_watch` | `a2m-provider` | A discoverer its manifest says a person drives was built without the method that watches a capture, so nothing was recorded. | Report this to the provider's author; a discoverer driven by a person implements `watch`. | no |
| `unknown_variant` | `document_type_refused` | `a2m-pack-http` | An XML answer that declares a document type. None is read, so no entity it defines is expanded and nothing it names is fetched (decision 0217). | Ask the system's owner for the answer without a document type, or read it another way. | no |
| `unknown_variant` | `download_incomplete` | `a2m-pack-web-ui` | The browser ended the download before the file was whole, or the step's time ended before it was. Nothing was kept. | Run the tool again; if it ends the same way, look at the page in a browser of your own. | no |
| `unknown_variant` | `download_not_offered` | `a2m-pack-web-ui` | The action declares a download, and the page began none before the step's time ended. | Look at the page; the action's click does not offer a file, or the file is offered another way than a download. | no |
| `unknown_variant` | `download_too_large` | `a2m-pack-web-ui` | The file the page offered is larger than the most the action declares. It was cancelled and nothing was kept. | Raise `download.most_bytes` of the action if the file is the one meant; otherwise look at what the page offers. | no |
| `unknown_variant` | `driver_panicked` | `a2m-engine` | A provider's code panicked. The engine goes on; the step does not. | Report this to the provider's author. | no |
| `unknown_variant` | `expect_failed` | `a2m-engine` | An `expect` does not hold. | Look at the record in the system; the tool runs only when its expectation holds. | no |
| `unknown_variant` | `field_not_read_back` | `a2m-engine` | Reading a record gave no value for a field the operation writes. | Make the binding's read-back return every field the operation writes. | no |
| `unknown_variant` | `field_read_only` | `a2m-pack-desktop` | A field takes no value today: it is read-only, or not editable. Nothing was set. | Look at the application; the field may be locked for this record. | no |
| `unknown_variant` | `finding_not_declared` | `a2m-provider` | A discoverer made a finding of a kind or level it did not declare. | Report this to the provider's author; a discoverer returns only the kinds and levels it declares. | no |
| `unknown_variant` | `fixture_no_answer` | `a2m-provider` | An offline test's fixture records no answer for a step. | Record the step's answer in the test's fixture, as a file named after it. | no |
| `unknown_variant` | `formula_at_place` | `a2m-pack-file` | A formula where a value was declared. | Mark the field `formula: true` to read its kept result, or point it at the cell that holds the value. | no |
| `unknown_variant` | `ledger_state_unexpected` | `a2m-ledger` | A transition no rule allows. | Read the run's state again before doing anything else. | no |
| `unknown_variant` | `metadata_not_kept` | `a2m-pack-file` | The old file's owner, group, or permissions cannot be carried to the new file. | Check who owns the folder and the file, and whether the engine's user may give the new file the old one's owner, group, and permissions. | no |
| `unknown_variant` | `net_answer_unreadable` | `a2m-netstate` | The system's answer has a shape this reader does not know, so it stops rather than guess. | Report the operating system and its version. | no |
| `unknown_variant` | `net_reader_not_built` | `a2m-netstate` | No reader of this fact is built for this operating system. | Read the fact in the system's own network settings; this reader is not built for this system yet. | no |
| `unknown_variant` | `no_read_back` | `a2m-engine` | A binding has no steps that read the record, so nothing it writes can be verified. | Give the binding a read-back, or declare the operation `irreversible`, and publish the target again. | no |
| `unknown_variant` | `output_unexpected` | `a2m-engine` | A step returned a value the compiled target gives no place to. | Compare the operation's outputs in the target files with what the system returns. | no |
| `unknown_variant` | `page_unreadable` | `a2m-pack-web-probe` | A page could not be read, in a way no rule covers. | Look at the page in a browser; report this if the page is an ordinary one. | no |
| `unknown_variant` | `physical_not_built` | `a2m-pack-device` | A step writes a register with the hazard `physical`, and such a write is not built. Nothing was sent. | Leave this operation out until `confirm_by` and the safe-state operation are built. | no |
| `unknown_variant` | `property_not_text` | `a2m-pack-desktop` | A service's property holds a value that is not text, a flag, a number, or an object path. The pack gives no other form. | Read another property. | no |
| `unknown_variant` | `read_too_large` | `a2m-pack-desktop`, `a2m-pack-web-ui` | A value is longer than the pack reads. | Read a field that holds less, or a smaller window. | no |
| `unknown_variant` | `reconcile_too_late` | `a2m-provider` | A committing step returned values for reconcile, which is too late. | Read the record back; then report this to the provider's author. | no |
| `unknown_variant` | `response_too_large` | `a2m-pack-http` | An answer larger than the limit. | Raise `max_response_bytes` on the endpoint, or `max_depth` or `max_elements` under its `answer`, if the answer is meant to be that large. | no |
| `unknown_variant` | `result_too_large` | `a2m-pack-data` | A statement's result holds more text than the limit allows. | Lower the statement's `max_rows`, or raise the pack's `result_bytes` setting. | no |
| `unknown_variant` | `runner_protocol_unsupported` | `a2m-sched` | A runner speaks a protocol version this scheduler does not. | Upgrade the runner or the control plane so that their versions are at most one apart. | no |
| `unknown_variant` | `session_move_illegal` | `a2m-auth` | The keeper was asked for a move that is not in the table of states. It is a fault of the keeper itself, and the move is not made. | Report it as a fault in the engine, with the session's timeline. | no |
| `unknown_variant` | `shape_unsupported` | `a2m-pack-file` | A shape of file, place, or value the pack has no rule for. | Change the document or the file so that the pack's rules cover it, or report the missing rule. | no |
| `unknown_variant` | `sign_in_unrecognized` | `a2m-pack-web-probe` | The surface's start page reads as a sign-in, and no rule of the web pack classifies how it asks for one. | Say how the surface signs in: declare it in `auth.yaml` with `draft.write`, or ask a person which method it is. | no |
| `unknown_variant` | `site_tools_unavailable` | `a2m-pack-web-ui` | The browser has no tools of pages to offer: the browser the engine owns does not carry the experimental domain they are called through, or a page of it has no place to register one (`document.modelContext`), as a page that is not a secure context has none. | Use a browser of a version that carries it, or bind the operation through the interface. | no |
| `unknown_variant` | `step_kind_unknown` | `a2m-provider` | A step of a kind its mode did not declare. | Compile the target again with the provider that is enabled now. | no |
| `unknown_variant` | `table_not_owned` | `a2m-ledger` | A statement named a table its crate does not own. | Reach the other crate's tables through its own interface. | no |
| `unknown_variant` | `tls_handshake_failed` | `a2m-auth`, `a2m-pack-http`, `a2m-pack-mcp` | The TLS handshake failed for a reason that is not the certificate: no version or cipher in common, or the far end ended it. | Check that the system offers TLS 1.2 or 1.3 at this address. | no |
| `unknown_variant` | `type_library_format_unknown` | `a2m-pack-desktop` | A file handed as a COM type library is in a form the reader does not know: the older form, a program file that may hold one, or something else. Only the MSFT form is read, and none is loaded by the system. | Hand a type library in the MSFT form, or leave it out of the discovery. | no |
| `unknown_variant` | `undeclared_code` | `a2m-provider` | A driver returned a failure code nothing declares. | Report this to the provider's author; every failure code is declared in the manifest. | no |
| `unknown_variant` | `unexpected_absent` | `a2m-provider` | A step that is no lookup reported its record absent. | Report this to the provider's author; only a lookup may find nothing. | no |
| `unknown_variant` | `unexpected_control` | `a2m-pack-desktop`, `a2m-pack-web-ui` | The element does not offer what the step needs: an editable text, or an action of the name the screen declares. | Take a snapshot of the window and correct the screen's field or the action's `perform`. | no |
| `unknown_variant` | `unexpected_dialog` | `a2m-pack-web-ui` | A dialog of the page that no screen declares. | Look at the page; a dialog is declared before a run may answer it. | no |
| `unknown_variant` | `unexpected_exit_status` | `a2m-pack-command` | A read ended with a status nothing declares, or by a signal. | Compare how the program ends now with the exits its command file declares, and correct the file if the program changed. | no |
| `unknown_variant` | `unexpected_page` | `a2m-pack-desktop`, `a2m-pack-web-ui` | The window a screen declares did not appear, or did not become ready, by the step's deadline. | Check that the application shows the window the screen declares, then run the tool again. | no |
| `unknown_variant` | `unexpected_prompt` | `a2m-pack-command` | A read's output holds a declared prompt. | Add the argument that makes the command not ask, such as `--yes`, to its `args` in `commands/`. | no |
| `unknown_variant` | `unexpected_reply` | `a2m-pack-device`, `a2m-pack-stream` | The device answered a read with a reply of no declared form. | Check that the surface names the protocol the system speaks, and the target's files against the system's documentation; then report this. | no |
| `unknown_variant` | `unexpected_response` | `a2m-auth` | The identity service answered with something that is not a token response. | Check the provider's settings in `auth.yaml` against the identity service's documentation. | no |
| `unknown_variant` | `unexpected_status` | `a2m-pack-data`, `a2m-pack-http`, `a2m-pack-mcp`, `a2m-pack-web-ui` | The store answered with an error no rule covers. | Look at the store's own log, then report the code to the pack's author. | no |
| `unknown_variant` | `unknown_control` | `a2m-pack-web-ui` | The compiled target names a control this driver has no rule for. | Compile the target again with the version of the web pack that is enabled. | no |
| `unknown_variant` | `value_kind_unsupported` | `a2m-pack-data` | A stored value is a float or bytes, which no rule reads. | Read the column through a statement that returns it as exact text, or leave the field out. | no |
| `unknown_variant` | `value_not_held` | `a2m-pack-desktop` | After a `set`, the field holds another value than the one set: the application changed it. Nothing was saved. | Give a value the field holds as it is written, such as a shorter one. | no |
| `unknown_variant` | `value_unreadable` | `a2m-pack-file` | A place holds something its field type cannot read. | Correct the value in the file, or the field's type in the document. | no |
| `unknown_variant` | `watch_condition_unanswered` | `a2m-ledger` | A watch's condition could not be answered on a check's result: its evaluation stopped, or it gave neither true nor false. Whether it holds is not known, so the check is a gap and is not compared. | Look at the check's run for what the read returned; the watch goes on and tries the condition on the next check. | no |
| `unknown_variant` | `watch_key_repeated` | `a2m-ledger` | A record's key appears twice in one check: a named fault of that check, never a merge. | Check the key the operation declares for its records; the watch goes on and compares the next check. | no |
| `unknown_variant` | `watch_result_not_its_shape` | `a2m-ledger` | A check's result is not of the shape the watch was asked with: its list output is missing or not a list of records, or a record lacks a field of its key. | Check the operation's outputs and the key it declares; the watch goes on and compares the next check. | no |
| `unknown_variant` | `watch_shapes_mixed` | `a2m-ledger` | A check holds keyed records and a whole result at once, or more than one whole result. | Check the operation's outputs; the watch goes on and compares the next check. | no |
| `rejected` | `app_validation_error` | `a2m-pack-command`, `a2m-pack-desktop`, `a2m-pack-http`, `a2m-pack-mcp`, `a2m-pack-web-ui` | An exit declared `rejected`. | Correct the values and call again. | no |
| `rejected` | `broker_refused` | `a2m-pack-stream` | The broker refused the message, with its reason. | Correct what the broker names, then send the message again. | no |
| `rejected` | `constraint_violated` | `a2m-pack-data` | A unique, check, not-null, or foreign-key constraint refused a write. | Correct the values so that they satisfy the store's constraints; nothing was written. | no |
| `rejected` | `decompiler_failed` | `a2m-provider` | A decompiling engine ended without success on the file it was handed. Nothing it printed was used. | Try the file again, or hand the engine a file it can read. | no |
| `rejected` | `device_refused` | `a2m-pack-device` | The device answered with an exception: an address or a value it will not take. | Check the register's address, layout, range, and access in the map against the device's documentation. | no |
| `rejected` | `image_refused` | `a2m-pack-device` | The device took the image into update mode and then refused it, or refused a block of it. It was told to leave update mode; the image it held in its other slot was given up when update mode began. | Check the image and the device's reason (the update error it reports) against the device's documentation; the device still runs its present firmware, and its other slot holds no image until an update is committed. | no |
| `rejected` | `run_not_completed` | `a2m-actions` | A run ended in a state other than completed, and carried no failure of its own. | Call `trace.show` with the run's identifier to see what happened. | no |
| `mismatch` | `image_checksum_differs` | `a2m-pack-device` | After the update the device reports a checksum of its image that is not the checksum the image carries. | Do not trust the device's firmware. Follow the recovery route the operation declares, then read the device's version and checksum before anything else is written. | no |
| `mismatch` | `read_back_differs` | `a2m-ledger`, `a2m-engine` | The digest read back is not the intended one. | Read the record again; if it still differs, stop and look at the evidence. | no |
| `possible_commit` | `ambiguous_match` | `a2m-pack-command`, `a2m-pack-mcp` | A lookup that must match one record matched more than one. | Find the records by hand, and decide which is the one that was written. | no |
| `possible_commit` | `answer_lost` | `a2m-local`, `a2m-mcp` | The engine may have had a request, such as a tool call, and its answer was lost: what the request asked may have been done, a write among it. | If the request carried an idempotency key, send the same request again with the same key and nothing changed: it gives back the run the first one started, finished or not, and starts no other. If it carried none, do not send it again, because a second request may start a second run and write twice: find its run first with the run list (`anything2mcp run list --since <ms>`, with `--tool <tool>` for a tool call), from one minute before the request was sent, and do what that run's `next` says. | no |
| `possible_commit` | `cannot_reconcile` | `a2m-engine` | A record in doubt cannot be read, so a person must settle it. | Look at the system, then cancel the run or settle the record by hand. | no |
| `possible_commit` | `device_not_returned` | `a2m-pack-device` | The update was committed and the device did not come back, or did not identify itself, within the restart window the map declares. Nothing is retried. | Alert a person and show the recovery route the operation declares; do not send the update again. | no |
| `possible_commit` | `no_confirmation` | `a2m-provider`, `a2m-engine`, `a2m-pack-device`, `a2m-pack-stream` | A write was sent and nothing says whether it happened. | Reconcile the record: read it back before anything is written again. | no |
| `possible_commit` | `reconcile_required` | `a2m-ledger` | A record of the run is still in doubt from an earlier lease and has not been read back. | Reconcile the run: read each record in doubt and settle it, then continue. | no |
| `possible_commit` | `runner_lost` | `a2m-ledger` | A call carries a lease number other than the run's, or a lease ended without a result, so a write may or may not have been sent. | Stop; wait for the run to be reconciled under its current lease, then resume it. | no |
| `stale` | `binding_changed` | `a2m-engine` | The binding a test or a trial named is no longer the one at its place: it moved, changed its mode, or changed what it does. | Run the test or the trial again against the drafts as they are now. | no |
| `stale` | `capture_ended` | `a2m-actions` | The capture was ended, by the one who asked for it, before its person began it on its page, so nothing was recorded. | Ask for a new capture with `capture.start`. | no |
| `stale` | `changed_since_preview` | `a2m-ledger`, `a2m-engine`, `a2m-pack-data`, `a2m-pack-file` | A record no longer matches what the preview saw. | Preview again and ask for the new diff to be approved. | no |
| `stale` | `drafts_changed` | `a2m-actions` | The drafts of a publish a person was shown changed before they pressed: they compile to another compiled target. | Ask again, and read the new page before pressing. | no |
| `stale` | `record_absent` | `a2m-pack-command` | A write's exit says the record it names is not there. | Read the record again; it may have been removed. | no |
| `stale` | `token_not_read` | `a2m-pack-http` | A write that sends an anti-forgery token, in a pass that fetched none for it, or whose token was already sent. Nothing is sent (decision 0357). | Run the tool again, so that the page is fetched and its token read before the write. | no |
| `stale` | `version_conflict` | `a2m-pack-command`, `a2m-pack-data`, `a2m-pack-http` | An exit declared `stale`. | Read the record again and decide whether the change still applies. | no |
| `stale` | `version_not_read` | `a2m-pack-http` | A write that sends a record's version as a precondition, in a pass that read no version for the record. Nothing is sent. | Preview the change again, so that the record is read before it is written. | no |
| `transient` | `connection_lost` | `a2m-provider`, `a2m-local`, `a2m-pack-device`, `a2m-pack-mcp`, `a2m-pack-stream` | A stream to a subsystem broke. | Try again; if the stream keeps breaking, run `anything2mcp doctor`. | yes |
| `transient` | `device_busy` | `a2m-pack-device` | The device answered that it is busy. Nothing was carried out. | Run the operation again when the device is free. | yes |
| `transient` | `engine_fault` | `a2m-api` | The dispatcher stopped without an answer. Served as 500. | Find the request's run with `run.list` before sending it again, unless it carried an Idempotency-Key header: then send it again with the same key. | yes |
| `transient` | `frame_corrupt` | `a2m-pack-device` | A reply was damaged, on a read. | Open the link again and read again. | yes |
| `transient` | `net_read_failed` | `a2m-netstate` | The system's call to read the fact failed. | Try again; if it fails again, report the operating system and the error number. | yes |
| `transient` | `nothing_arrived` | `a2m-pack-stream` | No message matched within the limit, and the operation needs one. | Try again when the message is expected, or raise the wait where the channel allows. | yes |
| `transient` | `page_not_loaded` | `a2m-pack-web-probe` | A page did not load. | Check that the surface's address is right and that the system is up, then run discovery again. | yes |
| `transient` | `prepare_timed_out` | `a2m-engine` | A driver was not ready within the time preparing may take. | Call the tool again. | yes |
| `transient` | `stdio_read_failed` | `a2m-mcp` | The proxy could not read its input. | Start the client again. | no |
| `transient` | `timeout` | `a2m-provider`, `a2m-pack-device`, `a2m-pack-mcp`, `a2m-pack-stream` | A deadline passed on something that commits nothing. | Try again; if the run was to write, read the record back before anything is written again. | yes |
| `transient` | `upstream_unavailable` | `a2m-auth`, `a2m-pack-command`, `a2m-pack-data`, `a2m-pack-http`, `a2m-pack-mcp`, `a2m-pack-stream` | The service did not answer, or answered that it cannot serve. | Try again in a moment; if it keeps failing, check the address in the target's files and the network. | yes |
| `busy` | `accessibility_off` | `a2m-pack-desktop` | The session's accessibility switch is off and the application publishes no tree (Linux), or the person has not allowed this program to use the system's accessibility interface (macOS). The pack changes no setting. | Turn on the desktop's accessibility setting, or allow the program that started the engine to use the system's accessibility interface, then run the tool again. | no |
| `busy` | `acknowledgement_not_kept` | `a2m-engine` | The control plane cannot end a record on the system's acknowledgement alone; the record stays `acted`, in doubt. | Look at the system and settle the record; the run stops until it is settled. | no |
| `busy` | `application_not_running` | `a2m-pack-desktop` | No running application is the program and the name the surface states. The pack starts nothing. | Start the application as you normally do, then run the tool again. | no |
| `busy` | `blocked_by_parent` | `a2m-engine` | A record waits on a parent record that failed. | Settle the earlier record, then resume the run. | no |
| `busy` | `browser_stopped_at_start` | `a2m-pack-web-browser`, `a2m-pack-web-ui` | The browser program started and stopped before it answered, and wrote why, such as a sandbox it cannot make (decision 0450). | Act on what the browser wrote, which the message shows. Where it names its sandbox, run the engine as a user that is not root, and let that user make user namespaces; in a container, give it a filter that allows them. The engine never starts a browser without its sandbox. | no |
| `busy` | `browser_unavailable` | `a2m-pack-web-browser`, `a2m-pack-web-ui` | No browser program was found. | Install Chrome, Edge, or Chromium, or name the browser program in the settings. | no |
| `busy` | `caller_queue_full` | `a2m-sched` | The caller has as many jobs waiting as it may. | Wait for one of the caller's jobs to start, or cancel one, then call again. | yes |
| `busy` | `capture_not_kept` | `a2m-engine` | The control plane keeps no captures; the capture was dropped. | Run on an engine that keeps captures if the capture is needed; the run goes on without it. | no |
| `busy` | `channel_unavailable` | `a2m-local` | The channel, a lock file, or the loopback listener cannot be opened on this machine. | Check that the engine's runtime and data directories exist and belong to you, then try again. | no |
| `busy` | `cleanup_incomplete` | `a2m-provider` | Leftovers of an earlier stop block a new start. | Run `anything2mcp doctor` to see what remains, end it, and try again. | no |
| `busy` | `control_unanswered` | `a2m-engine` | A call to the control plane was not answered within the time the engine waits for one. | See `anything2mcp engine status`, then resume the run. | yes |
| `busy` | `data_directory_gone` | `a2m-local` | The data directory is no longer the one the engine opened: its engine lock is gone, or another file stands in its place. The engine refuses the request and stops; the next client starts a new engine. | Run the command again: a new engine starts on the data directory as it is now. Runs and published targets that were in the removed directory are not in a new one. | no |
| `busy` | `decompiler_not_installed` | `a2m-provider` | No decompiling engine is installed where the settings say, or none is named. The scout reports it and goes on. | Install the engine yourself and name its program in the settings; the product never fetches one. | no |
| `busy` | `disk_full` | `a2m-provider` | The disk, or the user's share of it, is full. | Free space on the disk, then call again. | no |
| `busy` | `engine_degraded` | `a2m-engine`, `a2m-sched` | A write was asked of a kernel that takes reads only, or the kernel has stopped. | Run `anything2mcp doctor run`, repair what it names, then call again. | no |
| `busy` | `engine_running` | `a2m-local` | An engine already holds the engine lock. | Use the running engine, or stop it with `anything2mcp engine stop` first. | no |
| `busy` | `engine_unreachable` | `a2m-local` | Nothing answers on the local channel. | Start the engine with `anything2mcp serve`, then run the command again. If an engine runs and does not answer, read its log. | no |
| `busy` | `evidence_not_stored` | `a2m-engine` | Evidence could not be stored, so nothing can be marked verified. | Free space in the evidence store, then resume the run. | no |
| `busy` | `file_locked` | `a2m-pack-file` | The lock signal is present, or the system refuses to share the file. | Close the file in the other program, then try again. | no |
| `busy` | `job_kind_not_built` | `a2m-engine` | The job is of a kind this engine does not carry out yet. | Do the work as jobs of a kind the engine carries out, such as one operation after the other. | no |
| `busy` | `key_record_conflict` | `a2m-vault` | A key record already exists, or changed before it could be replaced. | Make the call again; it reads the record that was written first. | yes |
| `busy` | `key_store_entry_missing` | `a2m-vault` | The root that wrapped a key is no longer in the key store. | Sign in again to make each saved session anew; what was sealed under the lost root cannot be read. | no |
| `busy` | `key_store_unreachable` | `a2m-vault` | The key store does not answer. The kernel is degraded until it does. | Unlock or start the key store, then try again; run `anything2mcp doctor` to see which key store is in use. | no |
| `busy` | `lease_too_short` | `a2m-actions` | A step may take longer than a job's lease lasts. | Raise `runners.lease_seconds` above the tool's longest step time limits, then resume the run. | no |
| `busy` | `log_unreadable` | `a2m-logs` | A file of the engine's log is there and cannot be read. | Make the logs folder and its files readable by your user, then ask for the log again. | no |
| `busy` | `memory_short` | `a2m-pack-web-browser`, `a2m-pack-web-ui` | The engine's browsers hold their memory ceiling or more, or one of them could not be measured, so no new browser work is started (decision 0425). | Call again after the wait the failure gives; a browser past its own limit is recycled between runs. If it persists, close other work on the machine, or raise the ceiling in the settings. | yes |
| `busy` | `message_may_be_held` | `a2m-pack-stream` | A message delivered again may be held by the record that took it first; nothing was taken. | Settle the record that took the message first; the message is then taken by the next run. | no |
| `busy` | `no_binding` | `a2m-engine` | No binding qualifies, for a reason that is not a session, an edge block, or drift. | Check the binding's status and its provider with `anything2mcp engine status`, and enable a provider that drives a mode of the surface. | no |
| `busy` | `no_rate_budget` | `a2m-engine` | No rate budget was granted for a surface. | Call the tool again later. | yes |
| `busy` | `on_hold` | `a2m-sched` | The work is covered by a hold, such as the stop-everything switch. | Have whoever set the hold release it, then call again. | no |
| `busy` | `path_locked` | `a2m-provider` | Another program holds the file, or a lock on it. | Close the program that holds the file, then call again. | no |
| `busy` | `program_busy` | `a2m-pack-command` | An exit declared `busy`, or another start holds the surface. | Try again when the other start has ended. | yes |
| `busy` | `prompt_busy` | `a2m-local` | A presence prompt is already up on this machine, and one is raised at a time. | Answer or close that prompt, or let its page close, then run the command again. | yes |
| `busy` | `provider_unavailable` | `a2m-provider` | The provider is not ready here, now. | Run `anything2mcp doctor` to see what the provider needs, or name another provider for the mode. | no |
| `busy` | `queue_full` | `a2m-provider`, `a2m-sched` | A queue is full. | Call again after the wait the failure gives. | yes |
| `busy` | `randomness_unavailable` | `a2m-vault` | The operating system gave no random bytes, so no key or nonce was made. | Make the machine's random source available to the engine, then try again. | no |
| `busy` | `rate_limited` | `a2m-sched` | A rate budget is spent and does not refill inside the lease. | Call again after the wait the failure gives. | yes |
| `busy` | `reads_still_running` | `a2m-pack-http` | So many reads of answers are still running past their step's deadline, on this surface or in the pack, that no new one is begun. Nothing is sent (decision 0270). | Run the tool again once those reads have ended, or ask the endpoint's owner why its answers take so long to read. | yes |
| `busy` | `record_locked` | `a2m-ledger` | Another run holds the lock on the record. | Call again when that run has finished or been reconciled, after the wait the failure gives where it gives one. | yes |
| `busy` | `request_timed_out` | `a2m-api` | The dispatcher did not answer within the time allowed. The action may still finish. Served as 503. | Find the request's run with `run.list` before sending it again, unless it carried an Idempotency-Key header: then send it again with the same key. | yes |
| `busy` | `resource_in_use` | `a2m-sched` | A wait for an exclusive resource ran out. | Call again after the wait the failure gives. | yes |
| `busy` | `resource_order` | `a2m-sched` | A job asked for an exclusive resource that sorts before one it holds. | Release the resources that sort after the one asked for, then ask for all of them together. | no |
| `busy` | `run_call_unreadable` | `a2m-actions` | What a run was called with is missing from the store or cannot be read. | Call the tool again; that is a new run. Call `trace.show` with this run's identifier to see what it did before. | no |
| `busy` | `run_reconciling` | `a2m-sched` | A resume found the run's reconcile as its one job: what is in doubt is settled first. | Wait for the reconcile to end, then resume the run again. | yes |
| `busy` | `run_time_exceeded` | `a2m-engine` | The run passed the longest time its pipeline allows. | Resume the run; what it verified stays verified. | no |
| `busy` | `runner_unavailable` | `a2m-sched` | No runner can serve the job. | Start or enroll a runner that offers what the message names as missing, then call again. | no |
| `busy` | `service_not_running` | `a2m-pack-desktop` | No connection owns the bus name the surface states as its service (Linux). The pack starts nothing, whatever a `.service` file says. | Start the application as you normally do, then run the tool again. | no |
| `busy` | `session_in_use` | `a2m-sched`, `a2m-pack-desktop` | A wait for a place on a session ran out. | Call again when the work on that session has finished. | yes |
| `busy` | `shadow_not_kept` | `a2m-engine` | The control plane keeps no shadow comparisons, so no binding is run in shadow. | Compare a binding in shadow on an engine that keeps the comparisons; the call's result is the verified binding's. | no |
| `busy` | `shadow_time_exceeded` | `a2m-engine` | A shadow binding did not end within the time a shadow may take; it was cancelled and counted as not compared. | Call again; the shadow binding is compared on a later call. | yes |
| `busy` | `shadow_write_not_built` | `a2m-ledger` | The shadow of a binding of an operation that writes is not built: its comparison cannot be stored, and it is never promotable through shadow. | Keep the operation on its verified binding. | no |
| `busy` | `shutting_down` | `a2m-provider` | The supervisor is stopping and starts nothing. | Start the engine again. | no |
| `busy` | `start_failed` | `a2m-provider` | A program could not be started by the operating system. | Check that the program can be run on this machine, then try again. | no |
| `busy` | `step_not_built` | `a2m-pack-desktop` | A read of a management class's property or of a COM property get was drafted and compiles, and this version of the pack carries out no such step. | Use the read once a version of the desktop pack that carries out the step is enabled. | no |
| `busy` | `storage_full` | `a2m-logs`, `a2m-ledger`, `a2m-pack-file` | Something cannot be written, such as a log, a record of the ledger, or a file: the disk or the folder it goes to is full, or does not take the write. | Free space on the disk the message points to, or make its folder writable, then try again. | no |
| `busy` | `store_corrupt` | `a2m-ledger` | The integrity check, a stored row, a stored compiled target's hash, or the audit chain fails. | Stop writing, keep the data directory as it is, and restore the last backup. | no |
| `busy` | `store_locked` | `a2m-pack-data` | Another connection held the store's lock for the whole time limit. | Try again when the other program has finished its work. | yes |
| `busy` | `store_unavailable` | `a2m-ledger` | The database cannot commit, or the store is not ready. | Try again; if it lasts, run `anything2mcp doctor run` and repair what it names. | no |
| `busy` | `store_version_unsupported` | `a2m-ledger` | The store was written by a newer engine, or a migration it applied is not the one this engine carries. | Open the store with the engine version that last wrote it, or a newer one. | no |
| `busy` | `subscription_in_use` | `a2m-pack-stream` | The runtime's own subscription is held by another run. | Wait for the other run to end, then try again. | yes |
| `busy` | `subsystem_limit` | `a2m-provider` | As many subsystems of this kind run as may, or the place is taken. | Call again when one has finished, or raise the limit in the settings. | yes |
| `busy` | `surface_in_maintenance` | `a2m-pack-web-ui` | The surface answered 503: it is being worked on. Nothing is retried, and the surface is left alone for a while. | Try again after the maintenance ends. | yes |
| `busy` | `surface_unavailable` | `a2m-local` | The person surface cannot show a page: it holds no approval desk. | Run `anything2mcp doctor run` for the reason. | no |
| `busy` | `system_job_not_carried` | `a2m-actions` | The in-process runner loop was handed a job that is not a run's. | Report this; it is a fault in the engine. | no |
| `busy` | `system_not_built` | `a2m-pack-desktop` | This pack has no back end for the operating system it runs on. | Run the target on Linux, Windows, or macOS. | no |
| `busy` | `target_elevated` | `a2m-pack-desktop` | The application runs at a higher level than the engine, as administrator. Windows only; the pack does not reach it. | Start the application as yourself, without administrator rights, then run the tool again. | no |
| `busy` | `tool_changed` | `a2m-actions` | A published tool changed between the decision about a call and the call. | Call the tool again. | yes |
| `busy` | `watch_check_not_completed` | `a2m-ledger` | A check's run ended without completing and gave no failure: it was cancelled, or it expired. | Look at the check's run for why it ended; the watch goes on with the next check. | no |
| `busy` | `watch_check_running` | `a2m-ledger` | A check of the watch is under way already. | Wait for the check to end, then ask again. | yes |
| `busy` | `watch_check_unfinished` | `a2m-ledger` | A check's run has not reached an end the watch can take. | End the check when its run reaches a final state. | yes |
| `busy` | `watch_due_missed` | `a2m-ledger` | Due times passed with no check: the engine was not running, the watch was paused or held, or a check could not be admitted in time. A gap carries it. | Keep the engine running while a watch is due; the gap says which due times were missed. | no |
| `busy` | `watch_over_budget_share` | `a2m-sched` | The watches on one surface would take more of its budget than their share. | Ask for a longer interval, or end another watch on the same surface. | no |
| `busy` | `watch_paused` | `a2m-ledger` | The watch is paused. | Read the watch's changes for why it paused, and settle that. | no |
| `busy` | `watch_too_many` | `a2m-sched` | The person has as many watches as they may have. | End a watch, then ask again. | no |
| `busy` | `writes_not_built` | `a2m-pack-desktop` | This pack writes only on Linux, Windows, and macOS in this version. Nothing was sent. | Run the write on Linux, Windows, or macOS. On this system the pack reads only. | no |
<!-- failure-codes:end -->

### Meanings not yet stated in the code

Names the hand-kept list held before the table was generated, which no crate declares today. Each is kept here, by hand, so that nothing a reader had is gone; a name that a crate begins to declare moves into the generated table, and this row is then removed. The earlier list gave no meaning for any code, so there is none to carry over.

| Class named by the earlier list | Code |
| --- | --- |
| `edge_blocked` | `challenge_page` |
| `input` | `draft_under_release_version` |

## Command line

Every action in the [action catalog](action-catalog.md) is a command of the form `anything2mcp <noun> <verb>`, with a matching MCP tool, REST route, and SDK method. The nouns are `tool`, `run`, `session`, `target`, `auth`, `screen`, `capture`, `endpoints`, `spec`, `draft`, `test`, `fixture`, `shadow`, `version`, `publish`, `binding`, `toolset`, `grant`, `policy`, `runner`, `killswitch`, `audit`, `secret`, `vault`, `delegation`, `engine`, `config`, `doctor`, `net`, `reach`, `browser`, `plugin`, `client`, `trace`, `action`, `docs`, `self`, `provider`, `component`, `discover`, `finding`, `unknown`, and `scout`.

| Kind | Commands |
| --- | --- |
| Shortcuts | `init`, `add`, `learn`, `lint`, `try`, `test`, `ready`, `publish`, `status`, `call`, `run`, `diff`, `connect`, `doctor`, `docs`; a shortcut is offered only when every step is built, so `bundle`, `rollback`, and `migrate` wait for their actions |
| Process commands | `serve`, `runner`, `bridge`, `completion` |

On a local machine no command approves a write; see spec section 222. On a server, approvals are given in the console or a chat card after the approver signs in.

### Command output contract

| Item | Rule |
| --- | --- |
| Machine output | One JSON object on standard output with `--json` or when output is not a terminal |
| Success | `{"ok": true, "action", "result", "warnings", "next"}` |
| Failure | `{"ok": false, "error": {"code", "class", "message", "fix"}}` |
| `ok` | True when the answer carries no error and either it succeeded, or its exit class is person needed and it is not the answer of a run that rests on a person ([decision 0458](../decisions/0458-an-answer-that-did-its-part-and-waits-for-a-person-is-ok.md)) |
| Waiting for a person's act | `{"ok": true, "action", "result", "warnings", "next"}` with exit code 3 and the result's `state`: an authoring or admin action whose own part is done (`draft.ready`, `publish.request`, `binding.promote` of a write, `watch.create`, `capture.start`, `scout.run` stopped for a sign-in) |
| A run that rests on a person | `{"ok": false, "action", "result", "warnings", "next"}` with exit code 3 and no error: a write waiting for its approval, a run waiting for a sign-in or an answer, from `tool.call`, `run.get`, `run.resume`, or `action.run` of them. Nothing was written |
| Refused for want of a person | A failure, with exit code 3 |
| Exit codes | 0 success; 1 action failed; 2 malformed command; 3 a person is needed; 4 engine unreachable |
| Large results | Written under `.anything2mcp/out/`; the result carries path, size, and summary |
| Prompts | None when output is not a terminal |

## Local channel

| Item | Value |
| --- | --- |
| Transport | Unix domain socket (macOS, Linux); named pipe (Windows) |
| Access | Per-user directory or access list, plus a check of the connecting user on every connection |
| Protocol | HTTP carrying the `/api/v1` REST API and the MCP endpoint |
| What a command adds | One request header, read from the local channel alone: `A2M-Project`, the project folder it was run in, percent-encoded. No header says a person is at the caller's terminal: every caller of the channel is an agent ([decision 0158](../decisions/0158-the-product-crate-and-the-local-engine-after-their-safety-audit.md)). Every REST answer carries its exit class in the header `A2M-Exit-Class` ([decision 0131](../decisions/0131-the-front-doors-joined-serve-the-thin-client-and-the-bridge.md)) |
| Start | On demand, guarded by a lock file; `--in-process` for single-command use |
| Stop | After 30 minutes idle by default; session state saved to the vault |
| Version skew | A newer client asks an older engine to drain and restart |

## On-disk layout

Each operating system's own conventions are followed, as `Paths::resolve` in `a2m-actions` gives them. `A2M_HOME` overrides all of them with one directory. A folder named by a variable is used when the variable is set; the fallback is the one shown after "else".

| What | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Settings | `%APPDATA%\anything2mcp` | `~/Library/Application Support/anything2mcp` | `$XDG_CONFIG_HOME/anything2mcp`, else `~/.config/anything2mcp` |
| Store, blobs, browser profiles, logs (the data folder) | `%LOCALAPPDATA%\anything2mcp` | `~/Library/Application Support/anything2mcp` | `$XDG_DATA_HOME/anything2mcp`, else `~/.local/share/anything2mcp` |
| Runtime folder: the lock files, and the socket on macOS and Linux | `run` in the data folder; the channel is a named pipe carrying the user's identifier and a digest of this folder's path | `run` in the data folder, whatever `TMPDIR` says: it depends on `HOME` alone, and the system does not clean it ([decision 0432](../decisions/0432-the-runtime-folder-does-not-follow-a-variable-that-differs-between-one-users-processes.md)) | `$XDG_RUNTIME_DIR/anything2mcp`; else `/run/user/<uid>/anything2mcp` when `/run/user/<uid>` is a folder of the user's that no other account may enter, the folder the variable would have named; else `run` in the data folder (decision 0432) |
| Caches | `cache` in the data folder | `~/Library/Caches/anything2mcp` | `$XDG_CACHE_HOME/anything2mcp`, else `~/.cache/anything2mcp` |
| An administrator's installation file | `%PROGRAMDATA%\anything2mcp`, else `%LOCALAPPDATA%\anything2mcp` | `/Library/Application Support/anything2mcp` | `/etc/anything2mcp` |

Inside the data directory: `store/` for the database with `blobs/` for evidence inside it, `profiles/<target>/<environment>/<kind>-<digest>/` for one browser data directory per identity, `journal/` for the runner's local journal, `logs/`, and, on the passphrase route, `vault-salt`, the salt of the derivation with the algorithm and cost its root was derived with, which is not a secret ([decision 0438](../decisions/0438-the-passphrase-routes-salt-file-records-the-cost-its-root-was-derived-with.md)). A project folder holds only target files and working output, never any of these.
Inside the data directory: `engine.lock`, the data directory's half of the engine lock, and beside it `engine.runtime`, the runtime folder of the engine that holds the lock, written whole and readable by its user alone. A command refused `busy`/`engine_running` at the data directory's lock while its own runtime folder holds no engine names that folder, and on Linux the `XDG_RUNTIME_DIR` that leads to it, in the refusal's fix; it never connects there by the record ([decision 0432](../decisions/0432-the-runtime-folder-does-not-follow-a-variable-that-differs-between-one-users-processes.md)). Then `store/` for the database with `blobs/` for evidence inside it, `profiles/<target>/<environment>/<kind>-<digest>/` for one browser data directory per identity, `journal/` for the runner's local journal, and `logs/`. A project folder holds only target files and working output, never any of these.

## REST endpoints

| Method and path | Purpose |
| --- | --- |
| `GET /api/v1/tools` | List operations and pipelines the caller may use |
| `POST /api/v1/operations/{name}/calls` | Call an operation |
| `POST /api/v1/pipelines/{name}/runs` | Start a pipeline |
| `GET /api/v1/runs/{id}` | Run status and result |
| `POST /api/v1/runs/{id}/resume`, `/cancel` | Resume or cancel |
| `GET /api/v1/runs/{id}/events` | A run's events after a sequence number: one page as JSON, or a server-sent event stream that ends when the run settles. Not an action; only the caller that started the run reads them |
| `GET /api/v1/sessions` | Session status for the caller |
| `POST /api/v1/actions/{noun}.{verb}` | Run an authoring or admin action from the registry |
| `GET /api/v1/openapi.json` | The generated API description |

The two aliases of `tool.call`, `/operations/{name}/calls` and `/pipelines/{name}/runs`, are not served yet: the catalog gives `tool.call` no alias, so for now an operation or a pipeline is called through `POST /api/v1/actions/tool.call`.

## Plug-in protocol

A plug-in is a separate process that exchanges messages over standard input and output, started and stopped only by the supervisor. It receives only the inputs for its step. Secrets reach it only through `inject`, and only if an administrator approved the effects its manifest declares. The message list, its versioning, and the launch handshake are in the design of [`a2m-provider`](../architecture/crates/a2m-provider.md). The plug-in host and the framed link it speaks over are not built yet; a plug-in found on the machine is listed as present and not enabled.

## Benchmark matrix

Each cell is measured in CI against the built-in test application and compared with the previous release. The harness in `bench/` writes one JSON result per run (its schema is named "a2m-bench-result", version 1) and has two commands, `run` and `compare`.

| Scenario | Modes measured | Metric |
| --- | --- | --- |
| Single read | All | Overhead beyond app time, at the median and 99th percentile |
| Single write with read-back | All | Overhead; round trips used |
| 100-record pipeline | `ui`, `page_http`, `http` | Total time; records per second |
| 50 callers reading at once | `http`, `ui` | Throughput; queue wait |
| Cold start to first result | Solo profile | Wall time |
| Tool listing with 500 operations | MCP | Response time and size |
| Idle and loaded memory | Control plane, runner with 10 contexts | Resident memory |
