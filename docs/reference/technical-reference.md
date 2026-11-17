# Technical reference

This file holds the engineering detail behind the [system design](../spec/README.md): interfaces, data model, protocols, and catalogs. Everything here is a starting design to be refined in the phase that builds it.

## Crate layering

Crates depend only downward, which keeps build times short and the core small. A crate may depend only on the layers its row names, never on a crate in its own layer. `tools/docs_audit.py` reads this table, compares it with the crates on disk, and refuses any other edge. The reasoning behind each layer, and which crate defines each interface, are in [the architecture](../architecture/README.md) ([decision 0061](../decisions/0061-the-layers-are-redrawn-so-that-every-needed-dependency-is-allowed.md)).

Layers 1 to 8 are the kernel. Layers 9 and 10 are the packs ([spec section 202](../spec/2-kernel/202-kernel-and-packs.md)).

| Layer | Crates | May depend on |
| --- | --- | --- |
| 1. Types | `a2m-core` | Nothing internal |
| 2. Foundations | `a2m-expr`, `a2m-vault`, `a2m-telemetry` | Layer 1 |
| 3. Stores | `a2m-files`, `a2m-ledger` | Layers 1 to 2 |
| 4. Services | `a2m-policy`, `a2m-auth`, `a2m-sources` | Layers 1 to 3 |
| 5. Contract | `a2m-provider` | Layers 1 to 4 |
| 6. Engine | `a2m-engine`, `a2m-sched` | Layers 1 to 5 |
| 7. Actions | `a2m-actions` | Layers 1 to 6 |
| 8. Roles | `a2m-runner`, `a2m-mcp`, `a2m-api`, `a2m-local` | Layers 1 to 7 |
| 9. Pack bases | `a2m-pack-web-browser` | Layers 1 to 5 |
| 10. Packs | `a2m-pack-web-ui`, `a2m-pack-web-probe`, `a2m-pack-http`, `a2m-pack-mcp`, `a2m-pack-desktop`, `a2m-pack-command`, `a2m-pack-data`, `a2m-pack-file`, `a2m-pack-stream`, `a2m-pack-device` | Layers 1 to 5, and the base of its own pack |
| 11. Product | `anything2mcp` | Everything |

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
| `DigestPurpose` | Three: `blob_name`, `restricted_value`, `record_key` | [Decision 0069](../decisions/0069-the-plan-is-canonical-json-hashed-whole-and-keyed-digests-are-per-workspace.md) |

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
- **A checked name is declared with one macro,** `checked_name!`, which any crate may use for a name of its own. Each such type has `fixed`, which makes a name written in the source and fails the build when the name is badly formed.
- **`Value`** is what a field holds: nothing, true or false, a whole number, text, a decimal, a moment, a date, a length of time, a list, or a record. It is never a float and never a secret, and kinds are never converted. **`Values`** holds named values sorted by name, so the same values have the same bytes and the same digest whoever built them. **`ValueType`** describes a value's type, for the expression checker and the plan alike. **`Decimal`** is exact to 28 significant digits: a result that does not fit is refused, never rounded, and a division takes places and a rounding mode. **`RecordKey`** is one or more plain values in declared order.
- **Identifiers** are 16 bytes, ordered by the moment they were made, written as a one-letter prefix, an underscore, and 32 hexadecimal characters: `RunId` (`r_`), `JobId`, `SessionId`, `ApprovalId`, `DecisionId`, `WorkspaceId`, `PrincipalId`, `RunnerId`. The crate reads no clock and no random source; whoever makes one brings both. **`Identity`** is a person or a named account, never a credential.
- **`Plan`** is the compiled plan as data, with `encode`, `decode`, `check`, and `show`. It carries everything the run time needs from a target's files: beside the surfaces, field types, operations, pipelines, tools, and tests, the target's own facts, each environment's test records and approval rule, and how each group of surfaces is signed in to, with credentials as references only ([decision 0121](../decisions/0121-the-plan-carries-what-the-run-time-needs-from-target-files.md)). It is encoded as canonical JSON after one line that names the format; the hash is the digest of all the bytes, and `decode` checks it before it reads anything. A pack's own data is a `PackPayload`, held as canonical JSON text that the kernel never parses.
- **`Redact`** and **`KeyedDigest`** are the two interfaces the vault implements. `Redact` turns text into text with every registered secret removed, and redacts a whole failure or a set of values in one call. A keyed digest is separate for each workspace and each purpose, and has one construction, HMAC-SHA-256.

## Driver interface

The driver interface, the manifest, the registry, and the supervisor are built in `a2m-provider`. The [crate's design](../architecture/crates/a2m-provider.md) gives the interface in full, and decisions [0088](../decisions/0088-the-provider-contracts-outside-crates.md), [0089](../decisions/0089-the-provider-contract-as-building-it-settled-it.md), and [0090](../decisions/0090-the-supervisor-ends-a-group-only-while-its-leader-is-unreaped.md) record how building it settled it. The main types:

- **`Driver`** is what a pack implements for its modes, with the four calls below.
- **`DriverHandle`** is the only way the engine reaches a driver. The registry gives one for a mode, and it checks every step on the way in and every outcome on the way out.
- **`Authority`** is what a step is run under: the right to read, or the intent token for one write.
- **`StepInput`** is what one step is given, with its values already evaluated. **`StepOutcome`** is how a step ended when it did not fail.
- **`StepFailure`** is a failure together with **`Sent`**, which says whether the request was not sent, was answered, or is unknown.

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
- A step marked as a lookup may report that a record is absent. No other step may.
- A step's outcome can carry values known before the commit point, such as a key the store assigned. The engine records them before the committing step runs.

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

Each provider implements five operations: `sign_in`, `is_signed_in`, `detect_expiry`, `refresh`, `sign_out`, plus `inject`, which places credentials into a driver's context without returning them. Only the session keeper calls a provider. A provider also states what it needs and can do (a person, a silent renewal, a light prompt, state that can move, and whether a successful exchange confirms the credential) and checks an `auth.yaml` entry without signing in. The traits as built are in the [crate design](../architecture/crates/a2m-auth.md).

A provider lives in `a2m-auth` when it only holds or exchanges a credential. It lives in a pack when it needs a surface of the system. A provider that needs a person's consent in a browser, and is not in the web pack, gives the person a link to open in their own browser; nothing drives that browser.

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

## Vault interface

The design of [`a2m-vault`](../architecture/crates/a2m-vault.md) gives the interface in full. The main types:

- **`Vault`** seals a plain item into a **`Sealed`** envelope under a **`SealContext`**, and opens it again only under the same context. The caller stores the envelope.
- **`KeyStore`** is the back end that holds the root key. `OsKeyStore`, `PassphraseKeyStore`, and `MemoryKeyStore` implement it.
- **`KeyRecords`** is where wrapped keys persist, as one **`KeyRecord`** per key and **`Generation`**. `a2m-ledger` implements it.
- **`Redactor`** implements `Redact`, and **`VaultDigest`** implements `KeyedDigest`, both from `a2m-core`.

## Telemetry interface

The design of [`a2m-telemetry`](../architecture/crates/a2m-telemetry.md) gives the interface in full. The main types:

- **`TraceBuilder`** takes one **`StepRecord`** per step, redacts it, and ends as a **`FinishedTrace`**: the bytes and the trace signature. A trace that still holds a secret ends as a **`TraceRefused`**.
- **`Telemetry`**, made by `start` from **`TelemetrySettings`**, is the handle for spans, metrics, and the engine's own logs.
- **`Clock`** is how time is given, so that a test can fix it.

## Compiler registration points

The design of [`a2m-files`](../architecture/crates/a2m-files.md) gives the interface in full ([decision 0079](../decisions/0079-the-compilers-registration-points-and-the-file-formats-as-the-code-settles-them.md)). The main types:

- **`CompilerSetBuilder`** takes what each pack registers, under the pack's own name, and ends as a **`CompilerSet`**, which `compile` and `lint_one` are given.
- **`FileKind`**, **`Lowering`**, **`LintRule`**, and **`DraftingRule`** are the four traits a pack implements. A surface kind, as a **`SurfaceKindSpec`**, and a control name are registered as data.
- **`Compiled`** is a plan with its encoded bytes and its hash. **`Diagnostics`** is every problem found, each a **`Diagnostic`** with its place and a fix.

## Store interface

The design of [`a2m-ledger`](../architecture/crates/a2m-ledger.md) gives the interface in full (decisions [0080](../decisions/0080-the-store-is-one-sqlite-file-and-a-crate-reaches-its-own-tables-through-the-transaction.md) and [0081](../decisions/0081-what-the-ledger-holds-as-data-the-token-doubt-locks-and-rule-three.md)). The main types:

- **`Store`**, opened from a **`StoreConfig`**, owns the database, the migrations, and the blobs. Its `write` and `read` each run one closure as one transaction on the store's own thread, and hand the closure a **`Tx`**.
- **`Ledger`**, **`Findings`**, **`Versions`**, and **`Audit`** are the ledger's own tables inside a transaction. **`Blobs`** keeps evidence, sealed.
- **`Tables`**, from `tx.tables` with an **`Owner`**, is how a crate above reaches its own tables and no others.
- **`Fence`** names a run and its lease number. **`IntentToken`** is what the store gives for a stored intent, and what a step that commits must carry.

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
- **`LoadedSource`** is a source definition loaded from a plan. With it, `Sources` turns an input into **`ValidatedRecords`**, or refuses it as a **`Rejected`** that lists every **`Problem`**.

## Scheduler interface

The design of [`a2m-sched`](../architecture/crates/a2m-sched.md) gives the interface in full ([decision 0093](../decisions/0093-the-schedulers-interface-as-building-it-settled-it.md)). The main types:

- **`Scheduler`** admits a **`Submission`** and queues its job, hands a runner a **`JobLease`**, grants exclusive resources by **`ResourceKey`** and rate budgets by **`BudgetKey`**, and keeps holds by **`HoldScope`**.
- **`LockTicket`** is taken before the ledger is asked for a record lock, so that a wait cannot miss a release.
- **`SchedSettings`** holds the limits and their defaults. Nothing in the crate sleeps: `sweep` ends every wait whose moment has come.

## Engine interface

The design of [`a2m-engine`](../architecture/crates/a2m-engine.md) gives the interface in full. The main types:

- **`Engine`**, made from **`EngineDeps`**, carries out a job through `run_job`, and a discovery through `discover`, for the runner that holds the job's lease.
- **`JobSpec`** names the plan, the tool, the input, and the job's **`JobKind`**: a preview, an execute, a try, a reconcile, a check after a loss, a canary, or a discovery. The job ends as a **`JobOutcome`**.
- **`Control`** is everything the engine asks of the kernel around it: the plan, the guard's decision, a session lease, the preview, the approval, locks, intents, and the signed trace. In one process it is the action registry's `InProcessControl`.

## Action registry interface

The design of [`a2m-actions`](../architecture/crates/a2m-actions.md) gives the interface in full ([decision 0094](../decisions/0094-the-action-layers-interface-as-building-it-settled-it.md)). The main types:

- **`ActionRegistry`** holds every action of the catalog as data, built or not.
- **`Kernel`** is the handle every front door holds. Its `dispatch` takes a **`Request`** and gives a **`Response`**: it checks the input, the health state, and the person need, records one policy decision, runs the handler, and redacts the result.
- **`PersonSurface`** is how the kernel reaches a person. **`ApprovalDesk`** is where a person approves or declines a diff, and where an approval page reads what the diff would change (**`ApprovalPreview`**); no action reaches it. A decline ends the run as `expired`.
- **`Kernel::events`** and **`Kernel::next_events`** give a page of **`RunEvents`** after a sequence number, the second after waiting for one, to the caller that started the run ([decision 0132](../decisions/0132-the-action-registrys-second-pass-a-decline-ends-the-run-and-a-wait-reads-the-schedulers-own-table.md)).
- The faces are data generated from the registry: `command_faces`, `mcp_tools`, `rest_routes`, `sdk_methods`, and **`McpResultFace`**.

## REST router

The design of [`a2m-api`](../architecture/crates/a2m-api.md) gives the interface in full ([decision 0116](../decisions/0116-the-rest-router-takes-its-caller-from-the-mount-and-chooses-its-status-in-one-table.md)). The main items:

- **`router`** builds the routes from a `Kernel` and **`ApiSettings`**; every route calls the same `dispatch`.
- **`status_of`** is the one table from a response to an HTTP status. **`openapi`** generates the OpenAPI 3.1 document, and **`served_routes`** lists what the router serves.

## Data model

The same schema runs on SQLite and Postgres. Column-level definitions for the run, ledger, scheduling, and evidence tables are in the [ledger schema](ledger-schema.md).

| Table | Holds | Key columns |
| --- | --- | --- |
| `workspaces` | Isolation boundaries | id, name |
| `targets`, `target_versions` | Targets and their immutable published versions | target, version, hash, signature, compiled plan, status |
| `bindings_health` | Health of each binding | operation, mode, status, last check, shadow match counts |
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

Every failure carries a class, a stable code, a plain sentence, and a next step. The classes are closed. The codes below are examples: each crate names the codes it can produce, and its design in [the architecture](../architecture/README.md) lists them. The codes of `a2m-ledger` are listed in [its design](../architecture/crates/a2m-ledger.md). The rows include the codes of `a2m-engine`, `a2m-actions`, `a2m-api`, and `a2m-pack-web-browser`. Each pack's own codes, those of `a2m-pack-http`, `a2m-pack-command`, `a2m-pack-data`, `a2m-pack-file`, `a2m-pack-stream`, `a2m-pack-device`, and `a2m-pack-mcp`, are listed in its design and its manifest, and are not repeated here.

| Class | Example codes |
| --- | --- |
| `policy` | `not_granted`, `identity_not_allowed`, `effect_too_high`, `approval_missing`, `approval_stale`, `approval_refused`, `limit_exceeded`, `kill_switch_on`, `policy_evaluation_failed`, `presence_required`, `surface_not_declared`, `lease_superseded`, `runner_not_enrolled`, `effect_not_declared`, `signature_invalid`, `intent_missing`, `read_only_posture`, `plan_unverified`, `record_limit_exceeded`, `sandbox_read_only`, `intent_token_missing`, `person_required`, `caller_not_verified`, `setting_protected`, `setting_locked`, `caller_missing`, `password_read_refused` |
| `input` | `lint_failed`, `policy_tests_failed`, `decision_not_found`, `path_not_allowed`, `file_type_mismatch`, `schema_invalid`, `source_template_unknown`, `value_out_of_range`, `expression_invalid`, `expression_type_error`, `value_unparsable`, `value_absent`, `expression_cost_exceeded`, `plan_expression_unloadable`, `trace_holds_secret`, `telemetry_settings_invalid`, `vault_passphrase_mismatch`, `vault_passphrase_too_short`, `draft_conflict`, `registration_conflict`, `target_unreadable`, `formula_without_value`, `source_too_large`, `source_malformed`, `source_encoding_unsupported`, `source_protected`, `source_reader_unknown`, `source_definition_invalid`, `key_not_unique`, `auth_provider_unknown`, `auth_provider_duplicate`, `auth_settings_invalid`, `credential_missing`, `tls_not_enabled`, `manifest_invalid`, `contract_unsupported`, `mode_conflict`, `file_unavailable`, `provider_ambiguous`, `mode_unknown`, `not_registered`, `kind_owner_missing`, `kind_owner_conflict`, `provider_duplicate`, `pack_settings_invalid`, `program_not_found`, `job_invalid`, `input_invalid`, `action_not_built`, `client_action`, `settings_invalid`, `action_registry_invalid`, `body_too_large`, `content_type_unsupported`, `query_invalid`, `path_field_conflict`, `idempotency_key_invalid`, `route_not_found`, `method_not_allowed`, `body_timed_out`, `route_conflict`, `address_invalid` |
| `session_expired` | `needs_login`, `needs_mfa`, `device_required`, `headroom_short`, `lease_stale`, `credential_refused` |
| `edge_blocked` | `challenge_page`, `rate_limited_by_edge`, `forbidden_by_edge` |
| `drift` | `locator_not_found`, `fingerprint_mismatch`, `response_schema_mismatch`, `upstream_tool_changed`, `binding_drifted` |
| `unknown_variant` | `unexpected_dialog`, `unexpected_page`, `unexpected_status`, `runner_protocol_unsupported`, `unexpected_response`, `session_move_illegal`, `handshake_failed`, `undeclared_code`, `unexpected_absent`, `step_kind_unknown`, `reconcile_too_late`, `finding_not_declared`, `expect_failed`, `field_not_read_back`, `no_read_back`, `output_unexpected`, `commit_skipped`, `browser_not_owned`, `driver_panicked`, `condition_not_boolean` |
| `rejected` | `app_validation_error` |
| `mismatch` | `read_back_differs` |
| `possible_commit` | `no_confirmation`, `runner_lost`, `ambiguous_match`, `cannot_reconcile` |
| `stale` | `changed_since_preview`, `version_conflict` |
| `transient` | `timeout`, `connection_lost`, `upstream_unavailable`, `prepare_timed_out`, `engine_fault` |
| `busy` | `queue_full`, `record_locked`, `session_in_use`, `runner_unavailable`, `caller_queue_full`, `resource_in_use`, `resource_order`, `rate_limited`, `on_hold`, `storage_full`, `engine_degraded`, `randomness_unavailable`, `provider_unavailable`, `cleanup_incomplete`, `subsystem_limit`, `shutting_down`, `start_failed`, `no_binding`, `run_time_exceeded`, `blocked_by_parent`, `evidence_not_stored`, `no_rate_budget`, `job_kind_not_built`, `lease_too_short`, `request_timed_out`, `browser_unavailable`, `control_unanswered` |

## Command line

Every action in the [action catalog](action-catalog.md) is a command of the form `anything2mcp <noun> <verb>`, with a matching MCP tool, REST route, and SDK method. The nouns are `tool`, `run`, `session`, `target`, `auth`, `screen`, `capture`, `endpoints`, `spec`, `draft`, `test`, `fixture`, `shadow`, `plan`, `version`, `publish`, `binding`, `toolset`, `grant`, `policy`, `runner`, `killswitch`, `audit`, `secret`, `vault`, `delegation`, `engine`, `config`, `doctor`, `browser`, `plugin`, `client`, `trace`, `action`, `docs`, `self`, `provider`, `component`, `discover`, `finding`, `unknown`, and `scout`.

| Kind | Commands |
| --- | --- |
| Shortcuts | `init`, `add`, `learn`, `lint`, `try`, `test`, `publish`, `status`, `call`, `run`, `diff`, `bundle`, `rollback`, `migrate`, `connect`, `doctor`, `docs` |
| Process commands | `serve`, `runner`, `bridge` |

On a local machine no command approves a write; see spec section 222. On a server, approvals are given in the console or a chat card after the approver signs in.

### Command output contract

| Item | Rule |
| --- | --- |
| Machine output | One JSON object on standard output with `--json` or when output is not a terminal |
| Success | `{"ok": true, "action", "result", "warnings", "next"}` |
| Failure | `{"ok": false, "error": {"code", "class", "message", "fix"}}` |
| Exit codes | 0 success; 1 action failed; 2 malformed command; 3 a person is needed; 4 engine unreachable |
| Large results | Written under `.anything2mcp/out/`; the result carries path, size, and summary |
| Prompts | None when output is not a terminal |

## Local channel

| Item | Value |
| --- | --- |
| Transport | Unix domain socket (macOS, Linux); named pipe (Windows) |
| Access | Per-user directory or access list, plus a check of the connecting user on every connection |
| Protocol | HTTP carrying the `/api/v1` REST API and the MCP endpoint |
| What a command adds | Two request headers, read from the local channel alone: `A2M-Project`, the project folder it was run in, percent-encoded; and `A2M-Person-At-Terminal: true` when a person is at its terminal. Every REST answer carries its exit class in the header `A2M-Exit-Class` ([decision 0131](../decisions/0131-the-front-doors-joined-serve-the-thin-client-and-the-bridge.md)) |
| Start | On demand, guarded by a lock file; `--in-process` for single-command use |
| Stop | After 30 minutes idle by default; session state saved to the vault |
| Version skew | A newer client asks an older engine to drain and restart |

## On-disk layout

Each operating system's own conventions are followed. `A2M_HOME` overrides all of them with one directory. Exact paths are confirmed when the paths module is written in Phase 1.

| What | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Settings | Roaming application data | Application Support | The user's configuration directory |
| Store, blobs, browser profiles, logs | Local application data | Application Support | The user's data directory |
| Local channel and lock file | A named pipe carrying the user's identifier | A per-user runtime directory | The user's runtime directory |
| Pinned browsers and caches | Local application data | Caches | The user's cache directory |

Inside the data directory: `store/` for the database with `blobs/` for evidence inside it, `profiles/<target>/<environment>/<kind>-<digest>/` for one browser data directory per identity, `journal/` for the runner's local journal, and `logs/`. A project folder holds only target files and working output, never any of these.

## REST endpoints

| Method and path | Purpose |
| --- | --- |
| `GET /api/v1/tools` | List operations and pipelines the caller may use |
| `POST /api/v1/operations/{name}/calls` | Call an operation |
| `POST /api/v1/pipelines/{name}/runs` | Start a pipeline |
| `GET /api/v1/runs/{id}` | Run status and result |
| `POST /api/v1/runs/{id}/resume`, `/cancel` | Resume or cancel |
| `GET /api/v1/runs/{id}/events` | Server-sent event stream |
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
