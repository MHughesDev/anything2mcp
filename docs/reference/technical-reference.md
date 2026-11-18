# Technical reference

This file holds the engineering detail behind the [system design](../spec/README.md): interfaces, data model, protocols, and catalogs. A part that a package has built was settled by that package's decision record, and where this page and the code differ, the code is right. The rest is a starting design, refined by the package that builds it.

## Crate layering

Crates depend only downward, which keeps build times short and the core small. A crate may depend only on the layers its row names, never on a crate in its own layer. `tools/docs_audit.py` reads this table, compares it with the crates on disk, and refuses any other edge. The reasoning behind each layer, and which crate defines each interface, are in [the architecture](../architecture/README.md) ([decision 0061](../decisions/0061-the-layers-are-redrawn-so-that-every-needed-dependency-is-allowed.md)).

Layers 1 to 8 are the kernel. Layers 9 and 10 are the packs ([spec section 202](../spec/2-kernel/202-kernel-and-packs.md)).

| Layer | Crates | May depend on |
| --- | --- | --- |
| 1. Types | `a2m-core` | Nothing internal |
| 2. Foundations | `a2m-expr`, `a2m-vault`, `a2m-telemetry`, `a2m-netstate` | Layer 1 |
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
| `DigestPurpose` | Four: `blob_name`, `restricted_value`, `record_key`, `caller_key` | [Decision 0069](../decisions/0069-the-plan-is-canonical-json-hashed-whole-and-keyed-digests-are-per-workspace.md) |
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
- **A checked name is declared with one macro,** `checked_name!`, which any crate may use for a name of its own. Each such type has `fixed`, which makes a name written in the source and fails the build when the name is badly formed.
- **`Value`** is what a field holds: nothing, true or false, a whole number, text, a decimal, a moment, a date, a length of time, a list, or a record. It is never a float and never a secret, and kinds are never converted. **`Values`** holds named values sorted by name, so the same values have the same bytes and the same digest whoever built them. **`ValueType`** describes a value's type, for the expression checker and the plan alike. **`Decimal`** is exact to 28 significant digits: a result that does not fit is refused, never rounded, and a division takes places and a rounding mode. **`RecordKey`** is one or more plain values in declared order.
- **Identifiers** are 16 bytes, ordered by the moment they were made, written as a one-letter prefix, an underscore, and 32 hexadecimal characters: `RunId` (`r_`), `JobId`, `SessionId`, `ApprovalId`, `DecisionId`, `WorkspaceId`, `PrincipalId`, `RunnerId`. The crate reads no clock and no random source; whoever makes one brings both. **`Identity`** is a person or a named account, never a credential.
- **`Plan`** is the compiled plan as data, with `encode`, `decode`, `check`, and `show`. It carries everything the run time needs from a target's files: beside the surfaces, field types, operations, pipelines, tools, and tests, the target's own facts, each environment's test records and approval rule, and how each group of surfaces is signed in to, with credentials as references only ([decision 0121](../decisions/0121-the-plan-carries-what-the-run-time-needs-from-target-files.md)). It is encoded as canonical JSON after one line that names the format; the hash is the digest of all the bytes, and `decode` checks it before it reads anything. A pack's own data is a `PackPayload`, held as canonical JSON text that the kernel never parses.
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
- **`Destinations`** are the addresses a run may reach: the plan's surfaces and the operation's manifest, compared as written. Each carries the scope read from its own address when the set is made (`Destinations::scope_of`); nothing else sets it, and it decides nothing yet ([decision 0240](../decisions/0240-one-typed-reading-of-where-an-address-is-and-one-question-of-a-plain-link.md)).
- **`StepInput`** is what one step is given, with its values already evaluated. **`StepOutcome`** is how a step ended when it did not fail.
- **`StepFailure`** is a failure together with **`Sent`**, which says whether the request was not sent, was answered, or is unknown. What the system itself said travels on the failure, apart from its sentence (`StepFailure::with_said`).
- What a record's earlier steps recorded reaches a driver while it acts, as **`Prepared::recorded`**; how a step was done is the capture **`Capture::acting`**; the run's project folder is **`CheckedPaths::project_folder`**; a verified save in steps is **`CheckedPath::create_temporary`** and **`Temporary`** ([decision 0162](../decisions/0162-the-provider-contracts-second-pass-what-a-step-is-handed-and-hands-back.md)).
- A root bound to no folder, which stands for each request's project folder, is **`Roots::add_project`**; **`Roots::check_in`** and **`Roots::folder_in`** resolve it in the run's project folder ([decision 0193](../decisions/0193-the-core-the-contract-the-compiler-and-the-scheduler-after-the-second-pass.md)).
- A path resolved in the one form every root holds its folder in (on Windows the ordinary form `C:\...` where it names the same thing) is **`resolved_path`** of `a2m-provider`; whatever compares a path with a root's folder resolves it by this and by nothing else.
- **`Prepared::recorded_elsewhere`** asks the engine whether another record holds a value, and answers `Held`, `NotHeld`, or `CannotTell`, never the value ([decision 0193](../decisions/0193-the-core-the-contract-the-compiler-and-the-scheduler-after-the-second-pass.md)).
- **`SurfaceNotice`** (in `a2m-core`) is a closed list of the notices a door must show with a surface, such as unencrypted credentials; a kind registers one with `SurfaceKindSpec::with_notice`, and `CompilerSet::surface_notices` computes them ([decision 0193](../decisions/0193-the-core-the-contract-the-compiler-and-the-scheduler-after-the-second-pass.md)).
- **`ActionContext::paths`** is the call's own checked project folder, handed to a provider action; `DiscoveryScope::paths` is the run's ([decision 0196](../decisions/0196-the-engine-and-the-registry-after-the-acknowledgement-and-the-first-hour.md)).
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
- A step's outcome can carry values known before the commit point, such as a key the store assigned. The engine records them before the committing step runs.
- A failure of a committing step that was answered with class `policy` or `input` leaves the handle as `possible_commit`, `no_confirmation`, and `Sent::Unknown`: those two classes say nothing was sent, and on a write that left they are not the system's verdict ([decision 0148](../decisions/0148-the-provider-contract-and-the-scheduler-after-their-safety-audit.md)).
- The handle refuses a step that commits and is not marked, where the mode's manifest lists its kind in `commit_steps` (`policy`, `intent_missing`), and it lets no driver return a code in `codes::KERNEL_ONLY`, seven codes that state the kernel's bookkeeping. It redacts what a step returns, its values, its values for reconcile, and a capture whose bytes are text, as it does a failure.
- `CheckedPath` writes under a random name, only where nothing stands, and never through a link; the new methods are `entry_kind` (what stands at the final name, a link reported and not followed) and `create_dir` (one folder, new; it fails when anything is at the name). A link at the final name is refused for `write` and `remove_file`. A pack's manifest may not name a program by a relative path.
- The supervisor digests the program it opens at every start and refuses a digest that is none of the declared ones; on Linux it starts the file it digested.

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

## Vault interface

The design of [`a2m-vault`](../architecture/crates/a2m-vault.md) gives the interface in full. The main types:

- **`Vault`** seals a plain item into a **`Sealed`** envelope under a **`SealContext`**, and opens it again only under the same context. The caller stores the envelope.
- **`KeyStore`** is the back end that holds the root key. `OsKeyStore`, `PassphraseKeyStore`, and `MemoryKeyStore` implement it. A root, and the passphrase route's salt through `PassphraseSalt::load_or_create`, is made once, under a lock that two engines starting together both take.
- **`KeyRecords`** is where wrapped keys persist, as one **`KeyRecord`** per key and **`Generation`**. `a2m-ledger` implements it.
- **`Redactor`** implements `Redact`, and **`VaultDigest`** implements `KeyedDigest`, both from `a2m-core`. A text that arrives in pieces is redacted through the redactor's **`RedactStream`**, never piece by piece.

## Telemetry interface

The design of [`a2m-telemetry`](../architecture/crates/a2m-telemetry.md) gives the interface in full. The main types:

- **`TraceBuilder`** takes one **`StepRecord`** per step, redacts it, and ends as a **`FinishedTrace`**: the bytes and the trace signature. A trace that still holds a secret ends as a **`TraceRefused`**.
- **`Telemetry`**, made by `start` from **`TelemetrySettings`**, is the handle for spans, metrics, and the engine's own logs.
- **`Clock`** is how time is given, so that a test can fix it.

## Compiler registration points

The design of [`a2m-files`](../architecture/crates/a2m-files.md) gives the interface in full ([decision 0079](../decisions/0079-the-compilers-registration-points-and-the-file-formats-as-the-code-settles-them.md)). The main types:

- **`CompilerSetBuilder`** takes what each pack registers, under the pack's own name, and ends as a **`CompilerSet`**, which `compile` and `lint_one` are given.
- **`FileKind`**, **`Lowering`**, **`LintRule`**, and **`DraftingRule`** are the four traits a pack implements. A surface kind, as a **`SurfaceKindSpec`**, and a control name are registered as data. A lowering states, for each kind of step, whether it saves, as a **`StepCommits`** (`Never`, `Always`, `AsDeclared`), and the least effect a binding has by what it does; a file kind states whether a document describes a write, which the drafter never writes ([decision 0153](../decisions/0153-what-the-compiler-the-plan-and-the-redactor-hold-by-themselves-after-the-safety-audit.md)).
- **`CompilerSet::address_key`** gives the key under which a surface kind states its address, so that `target.add` can write a surface without a pack's knowledge in the registry ([decision 0199](../decisions/0199-the-halves-left-for-each-other-are-joined.md)).
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
- **`Scheduler::resume`** finds a run's one job and acts in the same transaction that would queue its own, and answers `Resumed::Queued` or `Resumed::Reconciling`; the second says how it is held and, for a door, is `busy`, `run_reconciling` through `Reconciling::failure()` ([decision 0201](../decisions/0201-a-resume-is-never-lost-the-browser-takes-no-proxy-and-no-product-build-speaks-a-stands-protocol.md)).
- **`LockTicket`** is taken before the ledger is asked for a record lock, so that a wait cannot miss a release.
- **`SchedSettings`** holds the limits and their defaults. Nothing in the crate sleeps: `sweep` ends every wait whose moment has come.

## Engine interface

The design of [`a2m-engine`](../architecture/crates/a2m-engine.md) gives the interface in full. The main types:

- **`Engine`**, made from **`EngineDeps`**, carries out a job through `run_job`, and a discovery through `discover`, for the runner that holds the job's lease.
- **`JobSpec`** names the plan, the tool, the input, and the job's **`JobKind`**: a preview, an execute, a try, a reconcile, a check after a loss, a canary, or a discovery. The job ends as a **`JobOutcome`**.
- **`Control`** is everything the engine asks of the kernel around it: the plan, the guard's decision, a session lease, the preview, the approval, locks, intents, and the signed trace. In one process it is the action registry's `InProcessControl`. Two of its calls have defaults: `other_records` answers what other records of an operation recorded, as a value that may have landed, and `step_observed` tells the registry what a reading step observed, which `fixture.record` holds in memory for its trial alone ([decision 0196](../decisions/0196-the-engine-and-the-registry-after-the-acknowledgement-and-the-first-hour.md)).

## Action registry interface

The design of [`a2m-actions`](../architecture/crates/a2m-actions.md) gives the interface in full ([decision 0094](../decisions/0094-the-action-layers-interface-as-building-it-settled-it.md)). The main types:

- **`ActionRegistry`** holds every action of the catalog as data, built or not.
- **`Kernel`** is the handle every front door holds. Its `dispatch` takes a **`Request`** and gives a **`Response`**: it checks the input, the health state, and the person need, records one policy decision, runs the handler, and redacts the result.
- **`PersonSurface`** is how the kernel reaches a person. **`ApprovalDesk`** is where a person approves or declines a diff, and where an approval page reads what the diff would change (**`ApprovalPreview`**); no action reaches it. A decline ends the run as `expired`.
- **`Kernel::events`** and **`Kernel::next_events`** give a page of **`RunEvents`** after a sequence number, the second after waiting for one, to the caller that started the run ([decision 0132](../decisions/0132-the-action-registrys-second-pass-a-decline-ends-the-run-and-a-wait-reads-the-schedulers-own-table.md)). **`Kernel::next_events_from`** waits from a **`Followed`**, what the follower last saw, and answers at once when the run came to rest or began again since ([decision 0159](../decisions/0159-a-lost-answer-is-a-write-in-doubt-at-the-doors-and-in-the-clients.md)).
- The faces are data generated from the registry: `command_faces`, `mcp_tools`, `rest_routes`, `sdk_methods`, and **`McpResultFace`**.

## REST router

The design of [`a2m-api`](../architecture/crates/a2m-api.md) gives the interface in full ([decision 0116](../decisions/0116-the-rest-router-takes-its-caller-from-the-mount-and-chooses-its-status-in-one-table.md)). The main items:

- **`router`** builds the routes from a `Kernel` and **`ApiSettings`**; every route calls the same `dispatch`.
- **`status_of`** is the one table from a response to an HTTP status. **`openapi`** generates the OpenAPI 3.1 document, and **`served_routes`** lists what the router serves.

## Data model

This is the data model as designed. The tables that exist are those the migrations in `crates/a2m-ledger/migrations/` create, which the [ledger schema](ledger-schema.md) describes column by column; a table named here and not there is not built. The migrations are SQLite's; Postgres is package 6.1 and is not built.

| Table | Holds | Key columns |
| --- | --- | --- |
| `workspaces` | Isolation boundaries | id, name |
| `targets`, `target_versions` | Targets and their immutable published versions | target, version, hash, signature, compiled plan, status |
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

Every failure carries a class, a stable code, a plain sentence, and a next step. The classes are closed: the twelve of `FailureClass`. A code is a checked name that belongs to exactly one class; a crate or a pack brings its own codes, and a code never changes once a release has carried it. Some codes are shared on purpose: the same code, with the same class, is declared by every crate that can produce it (for instance `connection_lost`, `timeout`, and `approval_stale`). A code declared with two classes is a fault, and the generator below refuses it.

The table is generated from the crates' own class tables (`ALL`, the `codes!` macros) and the providers' manifests (`failure_codes`), and from the constants and failure calls of the crates that keep no table, by `tools/failure_codes.py`. It must not be edited by hand: change the code, then run `python3 tools/failure_codes.py`. The documentation audit fails when the table and the crates differ. The last column is the sentence the crate's own doc comment gives the code; where several crates declare one code, the first crate listed that states a meaning supplies it, kernel crates before packs.

<!-- failure-codes:begin (generated by tools/failure_codes.py; do not edit by hand) -->
| Class | Code | Declared in | What it says |
| --- | --- | --- | --- |
| `policy` | `ack_not_owned` | `a2m-pack-stream` | An acknowledgement was asked for on a queue the runtime does not own. |
| `policy` | `address_looks_like_an_action` | `a2m-pack-web-probe` | Discovery was to visit an address that looks like an action. |
| `policy` | `application_not_owned` | `a2m-pack-desktop` | The program the surface states runs as another user, or on Windows in another session. Windows and macOS; the pack reads none of it. |
| `policy` | `approval_missing` | `a2m-ledger`, `a2m-policy` | Rule 3 finds no clearance for the run. |
| `policy` | `approval_refused` | `a2m-policy` | The person or the caller may not approve this run. |
| `policy` | `approval_stale` | `a2m-engine`, `a2m-ledger`, `a2m-policy` | The binding that would write a record is not one the approved diff names for it, or the stored diff is not the approved one. |
| `policy` | `caller_missing` | `a2m-api`, `a2m-mcp` | No caller came with the request: whoever mounts the router did not put one on it. status 401. |
| `policy` | `caller_not_verified` | `a2m-actions` | The caller's credential is not one this engine accepts. |
| `policy` | `connection_leads_elsewhere` | `a2m-pack-data` |  |
| `policy` | `denied_by_store` | `a2m-pack-data` | The store refused a write it holds no right to make. |
| `policy` | `device_serial_not_declared` | `a2m-pack-device` | The surface declares no serial for the device of the run's environment, so nothing is written to it. Nothing was sent. |
| `policy` | `discovery_identifies_only` | `a2m-pack-device` | A frame other than identification was asked for in the discovery posture. Nothing was sent. |
| `policy` | `effect_not_declared` | `a2m-provider` | A registration, a start, or a root beyond what the manifest declares. |
| `policy` | `effect_too_high` | `a2m-policy` | A grant exists, with a lower highest effect class. |
| `policy` | `engine_variable_refused` | `a2m-provider` | A launch names a variable that belongs to the engine. |
| `policy` | `file_protected` | `a2m-pack-file` | The file is encrypted, signed, or marked read-only. |
| `policy` | `file_too_large` | `a2m-pack-file` | The file is larger than the pack's limits allow. |
| `policy` | `folder_not_empty` | `a2m-pack-file` | A folder to remove is not empty. |
| `policy` | `frame_budget_exceeded` | `a2m-pack-device` | The run reached the most frames it may send on a link, or a write would leave too few for its read-back. Nothing was sent. |
| `policy` | `identity_is_administrator` | `a2m-pack-data` |  |
| `policy` | `identity_not_allowed` | `a2m-auth`, `a2m-policy` | The run does not act for the person the session belongs to. |
| `policy` | `intent_missing` | `a2m-provider` | A step that commits was given no intent token, or the wrong one. |
| `policy` | `intent_token_missing` | `a2m-engine` | An intent was stored and no token came back for it. |
| `policy` | `kill_switch_on` | `a2m-policy` | A kill switch covers the request. |
| `policy` | `lease_superseded` | `a2m-sched` | A call carries a lease that is not the job's current one, or one that has run out. |
| `policy` | `limit_exceeded` | `a2m-policy`, `a2m-pack-command` | Records per run, or a quota. |
| `policy` | `link_not_encrypted` | `a2m-pack-data`, `a2m-pack-http`, `a2m-pack-mcp` | A request that would carry the session's credential over plain `http` to an address that is not a loopback address, from a surface that does not accept it. Nothing is sent (decision 0188). |
| `policy` | `not_granted` | `a2m-policy`, `a2m-pack-mcp` | No permit applies to the caller, the tool, or the action. |
| `policy` | `offline_writes_nothing` | `a2m-actions` | An offline test asked for something that would store, lease, or clear a write; its control plane holds nothing and refuses (decision 0177). |
| `policy` | `page_write_refused` | `a2m-pack-web-ui` | The page sent a request that could change the system while a read ran, and the tab, which cannot write, failed it (decision 0195). |
| `policy` | `password_read_refused` | `a2m-pack-desktop`, `a2m-pack-web-browser`, `a2m-pack-web-ui` | A step would read the value of a control that holds a secret. |
| `policy` | `password_set_refused` | `a2m-pack-desktop` | A step would set a control that holds a secret. Sign-in to a desktop application is not built. |
| `policy` | `peer_not_allowed` | `a2m-local` | A connection from another user, or an engine owned by another user. |
| `policy` | `person_required` | `a2m-actions` | A person's act was asked for by a caller that cannot give one. |
| `policy` | `plan_unverified` | `a2m-engine` | The plan's bytes do not give the hash the job names. |
| `policy` | `policy_evaluation_failed` | `a2m-policy` | An error while deciding, or no valid policy set is loaded. |
| `policy` | `presence_failed` | `a2m-local` | The person dismissed or failed the presence prompt, or this machine has none. |
| `policy` | `presence_required` | `a2m-policy` | A change to a protected setting that only a person, present, may make. |
| `policy` | `program_path_unsafe` | `a2m-pack-command` | The path is not absolute, is a script, is a shell, or the file or a folder above it can be written by another user. |
| `policy` | `read_only_posture` | `a2m-provider`, `a2m-pack-data` | A step that commits was asked for where nothing may be written. |
| `policy` | `read_statement_wrote` | `a2m-pack-data` | A statement declared a read tried to change data. |
| `policy` | `record_limit_exceeded` | `a2m-engine` | The run would touch more records than its limit allows. |
| `policy` | `runner_not_enrolled` | `a2m-sched` | A runner that never said `hello` asked for work. |
| `policy` | `sandbox_read_only` | `a2m-engine` | A write was asked for where nothing may be written. |
| `policy` | `scan_inconclusive` | `a2m-pack-stream` | A look cannot say whether the record's message is in the log: the stream may lose a message from inside its range, or the range read was not whole. |
| `policy` | `scan_limit` | `a2m-pack-stream` | A log or a queue holds more than a scan may read, so the scan cannot say whether the record's message is there. |
| `policy` | `scout_off` | `a2m-actions` | The setting `authoring.scout.run` is `off`, and `scout.run` was called. |
| `policy` | `setting_locked` | `a2m-actions` | A higher layer tried to change a key the installation file locks. |
| `policy` | `setting_not_in_force` | `a2m-actions` | A protected setting is set to a value this build cannot put in force. |
| `policy` | `setting_protected` | `a2m-actions` | A layer that may only tighten tried to loosen a protected setting, or a layer set a key it may not set. |
| `policy` | `shadow_not_counted` | `a2m-ledger` | A comparison is not counted: the store is an offline test's, or the run it was made beside is not a live read under way. |
| `policy` | `shadow_not_promotable` | `a2m-ledger` | A person asked to promote a binding the evidence does not let them promote. |
| `policy` | `signature_invalid` | `a2m-provider` | A plug-in's signature does not verify. |
| `policy` | `statement_not_allowed` | `a2m-pack-data` | A statement is not one plain read or one keyed write: it attaches a file, changes a setting or the schema, ends a transaction, is several statements, or is a read that returns no column. |
| `policy` | `surface_not_declared` | `a2m-pack-data`, `a2m-pack-device`, `a2m-pack-http`, `a2m-pack-stream`, `a2m-pack-web-browser`, `a2m-pack-web-probe`, `a2m-pack-web-ui` | The link's address is not one the run may reach. |
| `policy` | `take_too_large` | `a2m-pack-stream` | A message taken is larger than may be recorded before it is acknowledged; it was given back. |
| `policy` | `transport_refused` | `a2m-pack-stream` | The connection would carry the session in plain text to an address that is not this machine's own, or to a server that asks for TLS. |
| `policy` | `vault_passphrase_not_enabled` | `a2m-vault` | The passphrase route was asked for and is not switched on. |
| `policy` | `watch_ended` | `a2m-ledger` | The watch has ended. |
| `policy` | `watch_needs_confirmation` | `a2m-ledger` | A paused watch cannot go on without a person confirming a new one: its version changed or its read drifted. |
| `policy` | `watch_not_a_read` | `a2m-ledger` | A watch was asked for an operation that is not a read. |
| `policy` | `watch_not_confirmed` | `a2m-ledger` | A check was asked of a watch its person has not confirmed. |
| `policy` | `watch_not_yours` | `a2m-ledger` | Someone other than the person a watch acts for tried to confirm it. |
| `policy` | `watch_run_not_its_own` | `a2m-ledger` | A run that is not the watch's own read was offered as its check. |
| `input` | `action_not_built` | `a2m-actions` | The action is in the catalog and its handler is not built yet. |
| `input` | `action_registry_invalid` | `a2m-actions` | The registry of actions cannot be built: a duplicate name, or a provider's action outside its name space. |
| `input` | `address_invalid` | `a2m-pack-web-browser`, `a2m-pack-web-ui` | An address is not one a tab may be sent to. |
| `input` | `address_value_refused` | `a2m-pack-web-browser`, `a2m-pack-web-ui` | A value cannot stand in one segment of a path: it is refused, never encoded. |
| `input` | `argument_unsafe` | `a2m-pack-command` | A value holds a NUL character, or would become an option. |
| `input` | `arguments_invalid` | `a2m-pack-mcp` | The arguments of a call do not fit the tool's pinned input schema. |
| `input` | `auth_provider_duplicate` | `a2m-auth` | Two providers were given the same name. |
| `input` | `auth_provider_unknown` | `a2m-auth` | An auth entry names a provider that nobody registered. |
| `input` | `auth_scheme_not_built` | `a2m-pack-http` | The session's provider needs a scheme this pack does not carry out yet. |
| `input` | `auth_settings_invalid` | `a2m-auth` | An auth entry lacks a setting its provider needs, or holds a wrong one. |
| `input` | `binding_not_promotable` | `a2m-actions` | A binding `binding.promote` may not make verified: not a drafted first binding of its operation, or one whose file cannot be rewritten by rule (decision 0189). |
| `input` | `body_timed_out` | `a2m-api` | The body did not arrive within the time allowed. 408. |
| `input` | `body_too_large` | `a2m-api` | The body is larger than the limit, or ended early. 413. |
| `input` | `client_action` | `a2m-actions` | The action is carried out by the client end, which did not do so. |
| `input` | `client_config_refused` | `a2m-actions` | `client.connect` cannot set a client's configuration: its profile does not read, names no file for this system, or the client's file does not read as a JSON object (decision 0219). Nothing is written. |
| `input` | `content_type_unsupported` | `a2m-api` | A body was sent with a type other than JSON. 415. |
| `input` | `contract_unsupported` | `a2m-provider` | A manifest was written for a contract this build does not accept. |
| `input` | `credential_missing` | `a2m-auth` | A credential reference names nothing that is stored. |
| `input` | `credential_not_carried` | `a2m-pack-data`, `a2m-pack-stream` | The session holds a credential the wire protocol cannot present. |
| `input` | `decision_not_found` | `a2m-policy` | No decision with that identifier is recorded. |
| `input` | `description_too_large` | `a2m-pack-http` | A published description that is larger, deeper, or wider once its references are followed than the import reads. |
| `input` | `description_unreadable` | `a2m-pack-http` | A published description that cannot be read. |
| `input` | `draft_conflict` | `a2m-files` | The drafter would overwrite a file the developer changed. |
| `input` | `expression_cost_exceeded` | `a2m-expr` | Evaluation passed its cost limit. |
| `input` | `expression_invalid` | `a2m-expr` | The text does not parse, or names something the environment does not declare. |
| `input` | `expression_type_error` | `a2m-expr` | A type does not fit, including any use of a float. |
| `input` | `file_type_mismatch` | `a2m-sources` | The content is not the format its reader reads. |
| `input` | `file_unavailable` | `a2m-provider` | A file under a checked path could not be read or written. |
| `input` | `finding_invalid` | `a2m-core` | A finding claims more than its source, its parents, or its content allow. |
| `input` | `fixture_no_scrub_rule` | `a2m-actions` | `fixture.record` met an observed value that no scrubbing rule covers, and wrote nothing (decision 0196). |
| `input` | `fixture_not_replayed` | `a2m-actions` | The test `fixture.record` made does not pass against its scrubbed fixture, or an observed value would be in a file; nothing was written. |
| `input` | `fixture_read_failed` | `a2m-actions` | The read `fixture.record` runs did not complete in the sandbox, or gave nothing to record; nothing was written. |
| `input` | `focus_invalid` | `a2m-pack-web-probe` | The focus of a discovery is not in the form this pack reads. |
| `input` | `formula_without_value` | `a2m-sources` | A formula cell in the data has no cached value. |
| `input` | `hello_invalid` | `a2m-local` | A connection's first request was not a well-formed hello. |
| `input` | `idempotency_key_invalid` | `a2m-api`, `a2m-ledger` | The `Idempotency-Key` header is empty, too long, or not printable text. |
| `input` | `idempotency_key_reused` | `a2m-actions` | An idempotency key the caller already gave with another call: another tool, environment, input, or source. |
| `input` | `input_invalid` | `a2m-engine` | An input is missing, is not declared, or is not of its type. |
| `input` | `job_invalid` | `a2m-engine` | The job names a tool, an environment, or a surface the plan does not hold, or a job of a kind the tool cannot run as. |
| `input` | `kind_owner_conflict` | `a2m-provider` | Two enabled providers own one kind for one surface kind. |
| `input` | `kind_owner_missing` | `a2m-provider` | A kind a provider reads has no enabled owner. |
| `input` | `lint_failed` | `a2m-files` | Any error diagnostic: the files do not compile. |
| `input` | `local_api_mismatch` | `a2m-local` | The client and the engine speak different versions of the local API. |
| `input` | `manifest_invalid` | `a2m-provider` | A manifest does not parse, or a field of it is wrong. |
| `input` | `mcp_not_served` | `a2m-mcp` | The service was asked for a toolset or a revision it does not serve. |
| `input` | `message_too_large` | `a2m-pack-stream` | A message is larger than its channel allows. |
| `input` | `method_not_allowed` | `a2m-api` | The route exists and does not take this method. 405. |
| `input` | `mode_conflict` | `a2m-provider` | A mode or surface kind is registered twice, or with two archetypes. |
| `input` | `mode_unknown` | `a2m-provider` | Nobody registered the mode. |
| `input` | `name_case_conflict` | `a2m-pack-file` | A folder holds a name that differs from the one given only in case. |
| `input` | `not_built` | `a2m-pack-mcp` | A part of the pack's design that this build does not carry out. |
| `input` | `not_found` | `a2m-ledger` | The thing asked for is not in the store. |
| `input` | `not_registered` | `a2m-provider` | Nobody registered the reader, action, or discoverer asked for. |
| `input` | `nothing_recorded` | `a2m-pack-stream` | A step that settles a record by what its earlier steps recorded was given nothing recorded, or nothing of the form it needs; nothing was sent. |
| `input` | `pack_settings_invalid` | `a2m-provider` | A pack's settings group does not fit the schema its manifest declares. |
| `input` | `path_denied` | `a2m-provider` | The engine's user may not write the file, or the file system is read only. |
| `input` | `path_field_conflict` | `a2m-api` | A field given in the path was given again, differently, in the body. |
| `input` | `path_missing` | `a2m-provider` | A file, or the folder that holds it, is not there. |
| `input` | `path_not_allowed` | `a2m-actions`, `a2m-files`, `a2m-provider`, `a2m-pack-data` | A project or file path is outside the allowed roots. |
| `input` | `path_value_refused` | `a2m-pack-http` | A value that cannot stand in one segment of a path. |
| `input` | `payload_not_encodable` | `a2m-core` | Data holds something the plan's encoding cannot carry, such as a float. |
| `input` | `payload_unreadable` | `a2m-core`, `a2m-pack-data`, `a2m-pack-device`, `a2m-pack-file`, `a2m-pack-mcp`, `a2m-pack-stream` | A payload in a plan does not decode as the type its reader expects. |
| `input` | `plan_expression_unloadable` | `a2m-expr` | A plan's expression names a library this engine does not have, no longer checks as the type it states, or was run without a name it reads. |
| `input` | `plan_format_unsupported` | `a2m-core` | A plan's format is not one this engine loads. |
| `input` | `plan_hash_mismatch` | `a2m-core` | A plan's bytes do not match the hash it was published with. |
| `input` | `plan_unreadable` | `a2m-core` | A plan's bytes match its hash and are not a well-formed plan. |
| `input` | `policy_tests_failed` | `a2m-policy` | A policy set whose own test table does not pass. |
| `input` | `program_not_found` | `a2m-provider` | A program to start was not found where it was declared. |
| `input` | `project_folder_missing` | `a2m-provider` | The run has no project folder, and a file of the project was asked for. |
| `input` | `promotion_unproven` | `a2m-actions` | A promotion whose evidence is missing: an offline test of the operation that is missing or did not pass, or a read no trial proved at `executed` (decision 0189). |
| `input` | `protocol_unknown` | `a2m-pack-stream` | The surface names a wire protocol this build does not speak. |
| `input` | `provider_ambiguous` | `a2m-provider` | Two ready providers drive one mode and no setting names one. |
| `input` | `provider_duplicate` | `a2m-provider` | Two providers carry one name. |
| `input` | `publish_gates_not_passed` | `a2m-actions` | A publish whose offline tests did not all run and pass: a version is published only when they do (spec section 403, decision 0180). the target's files are what a caller corrects. |
| `input` | `query_invalid` | `a2m-api` | The query of a request is not well formed, names a field twice, or was sent on a route that reads its input from the body. |
| `input` | `registration_conflict` | `a2m-files` | Two registrations for the same key, or one that is not the registering pack's to make. |
| `input` | `route_conflict` | `a2m-api` | Two routes the registry describes cannot both be served: the same method and path, or a path field named differently at the same place. Returned by [`crate::router`], never to a caller. |
| `input` | `route_not_found` | `a2m-api` | No route has this path. 404. |
| `input` | `schema_invalid` | `a2m-core`, `a2m-pack-command`, `a2m-pack-device`, `a2m-pack-stream` | A file, an input, or a name does not have the shape its schema requires. |
| `input` | `sealed_format_unsupported` | `a2m-vault` | A sealed item's format or cipher is one this engine does not know. |
| `input` | `sealed_item_mismatch` | `a2m-vault` | A sealed item does not open under the context given. |
| `input` | `settings_invalid` | `a2m-actions` | A settings file, variable, or flag cannot be loaded. |
| `input` | `socket_path_too_long` | `a2m-local` | The engine's socket would have a path longer than the system takes for a socket's address, so the channel can be neither made nor reached there. |
| `input` | `source_definition_invalid` | `a2m-sources` | The compiled source definition does not hold together. |
| `input` | `source_encoding_unsupported` | `a2m-sources` | The text is in an encoding the readers do not read. |
| `input` | `source_malformed` | `a2m-sources` | The content claims its format and breaks the format's rules. |
| `input` | `source_protected` | `a2m-sources` | The workbook is encrypted, so it cannot be read without its password. |
| `input` | `source_reader_unknown` | `a2m-sources` | The definition names a reader that is not registered. |
| `input` | `source_template_unknown` | `a2m-sources` | The input's structure matches no template version the definition knows. |
| `input` | `source_too_large` | `a2m-sources` | The bytes, the rows, the cells, or one cell exceed the caller's limits. |
| `input` | `step_not_usable` | `a2m-pack-command`, `a2m-pack-desktop`, `a2m-pack-http`, `a2m-pack-web-ui` | A step this driver cannot carry out as the plan wrote it: a command that writes on a step that is no commit point, or a step whose program was not started first. |
| `input` | `target_unreadable` | `a2m-files` | The target folder could not be read. |
| `input` | `telemetry_settings_invalid` | `a2m-telemetry` | The settings given to `start` cannot be used. |
| `input` | `tls_not_enabled` | `a2m-auth`, `a2m-pack-http`, `a2m-pack-mcp` | This build cannot make HTTPS requests, and an identity service needs one. |
| `input` | `tls_setup_failed` | `a2m-auth`, `a2m-pack-http`, `a2m-pack-mcp` | The TLS configuration of this machine could not be made. |
| `input` | `trace_holds_secret` | `a2m-telemetry` | A finished trace still held a registered secret at the check on the way out. The trace is dropped and never reaches the store. |
| `input` | `trust_roots_unreadable` | `a2m-auth` | A file of further trust roots holds no certificate, or one that cannot be read. |
| `input` | `value_absent` | `a2m-expr` | A value was absent where the expression needs one. |
| `input` | `value_begins_formula` | `a2m-pack-file` | A value for delimited text begins with a character that makes a spreadsheet program run the text as a formula. |
| `input` | `value_not_bindable` | `a2m-pack-data` | An argument is missing, or is a list or a record. |
| `input` | `value_out_of_range` | `a2m-core`, `a2m-expr`, `a2m-pack-device`, `a2m-pack-file`, `a2m-pack-stream` | A decimal, a date, a key part, or a moment cannot be represented. |
| `input` | `value_unparsable` | `a2m-expr`, `a2m-pack-file` | A parse function met text it cannot read. |
| `input` | `vault_passphrase_mismatch` | `a2m-vault` | The passphrase given is not the one the keys were wrapped under. |
| `input` | `vault_passphrase_missing` | `a2m-vault` | The passphrase route is chosen and no passphrase was given. |
| `input` | `vault_passphrase_too_short` | `a2m-vault` | The passphrase given is too short to derive a root key from. |
| `input` | `watch_interval_too_short` | `a2m-sched` | A watch's interval is below the floor. |
| `input` | `watch_too_long` | `a2m-sched` | A watch's end is further away than the longest a watch may last. |
| `session_expired` | `credential_refused` | `a2m-auth` | A stored credential was refused by the side that checks it. |
| `session_expired` | `device_required` | `a2m-auth` | The session's credentials are held on a runner that is not this one. |
| `session_expired` | `headroom_short` | `a2m-auth` | The session will end before the time a pipeline asked to have left. |
| `session_expired` | `lease_stale` | `a2m-auth` | The lease was granted for credentials that have since been replaced. |
| `session_expired` | `needs_login` | `a2m-auth`, `a2m-engine`, `a2m-pack-stream`, `a2m-pack-web-probe` | The session is absent, expired, or revoked, or a wait for it ran out. |
| `session_expired` | `needs_mfa` | `a2m-auth` | A second factor is demanded and no person answered in time. |
| `edge_blocked` | `edge_blocked` | `a2m-engine` | The surface is blocked at the edge. |
| `edge_blocked` | `forbidden_by_edge` | `a2m-pack-web-probe` | The surface answered a page with status 403. |
| `edge_blocked` | `net_fact_refused` | `a2m-netstate` | The system refused this fact to this user. |
| `edge_blocked` | `rate_limited_by_edge` | `a2m-pack-web-probe` | The surface answered a page with status 429. |
| `edge_blocked` | `sign_in_blocked` | `a2m-pack-web-ui` | The sign-in page of a form sign-in shows the block the target declares: a locked account, a check the engine does not get past (decision 0245). |
| `drift` | `anti_forgery_not_found` | `a2m-pack-web-ui` | The page a write is made from holds no anti-forgery token where its entry says. Nothing was sent (decision 0206). |
| `drift` | `binding_drifted` | `a2m-engine` | Every binding that could carry the call out has drifted. |
| `drift` | `channel_missing` | `a2m-pack-stream` | The channel, or the runtime's own subscription on it, is gone. |
| `drift` | `file_missing` | `a2m-pack-file` | The file is not at its declared path. |
| `drift` | `file_unreadable` | `a2m-pack-file` | The file does not parse as its declared format. |
| `drift` | `fingerprint_mismatch` | `a2m-pack-desktop`, `a2m-pack-file`, `a2m-pack-web-ui` | A table's headings are not the ones its screen declares, or a row of it does not have one cell for each heading. |
| `drift` | `key_not_unique` | `a2m-sources`, `a2m-pack-data`, `a2m-pack-file` | Two rows have the same key. |
| `drift` | `locator_ambiguous` | `a2m-pack-desktop`, `a2m-pack-web-ui` | A locator matched more than one element, or a screen's window matched more than one window. The first is never picked. |
| `drift` | `locator_not_found` | `a2m-pack-desktop`, `a2m-pack-web-ui` | No locator of a field or a table matched. |
| `drift` | `output_unparsed` | `a2m-pack-command` | A read's output does not parse as its declared form. |
| `drift` | `place_not_found` | `a2m-pack-file` | A named place, a header, a table, a name, or a key is not in the file. |
| `drift` | `position_out_of_range` | `a2m-pack-stream` | A position the run was to read from is no longer held. |
| `drift` | `program_digest_changed` | `a2m-pack-command` | The file's digest is not one listed for this platform. |
| `drift` | `program_missing` | `a2m-pack-command` | The program's path is missing, is not a file, or cannot be run. |
| `drift` | `program_version_unsupported` | `a2m-pack-command` | The version is absent, does not match, or is not a supported one. |
| `drift` | `read_not_by_key` | `a2m-pack-data` | A read of one record by its key gave a record with another key, or does not return the key. |
| `drift` | `reply_changed` | `a2m-files` |  |
| `drift` | `response_schema_mismatch` | `a2m-pack-http`, `a2m-pack-mcp`, `a2m-pack-stream`, `a2m-pack-web-ui` | The body of an accepted answer does not fit the declared shape. |
| `drift` | `round_trip_failed` | `a2m-pack-file` | The patch editor would change something it was not asked to change. |
| `drift` | `schema_changed` | `a2m-pack-data` | A table, column, parameter, or trigger differs from the plan. |
| `drift` | `shape_changed` | `a2m-pack-stream` | The channel's shape or key is not the one its file declares. |
| `drift` | `store_file_replaced` | `a2m-pack-data` | The embedded store's file was replaced by another file at its name after the run was prepared. |
| `drift` | `upstream_tool_changed` | `a2m-pack-mcp` | A mapped tool's definition is not the one its mapping pins, or the tool is gone. |
| `unknown_variant` | `anti_forgery_unreadable` | `a2m-pack-web-ui` | The page holds two different anti-forgery tokens where its entry says, or one of a form that is not sent. Nothing was sent. |
| `unknown_variant` | `application_ambiguous` | `a2m-pack-desktop` | More than one running application is the program and the name the surface states. |
| `unknown_variant` | `application_not_proven` | `a2m-pack-desktop` | The running application's program is not the one whose digest the surface states. |
| `unknown_variant` | `argument_masked` | `a2m-engine` | An argument of a step holds the mask of a value the redactor removed. |
| `unknown_variant` | `browser_not_owned` | `a2m-pack-web-browser`, `a2m-pack-web-ui` | The program that started is not a browser this crate can own. |
| `unknown_variant` | `catalog_too_large` | `a2m-pack-data` | The store's catalog holds more tables than one read takes, so what a write reaches cannot be shown. |
| `unknown_variant` | `certificate_expired` | `a2m-auth`, `a2m-pack-http`, `a2m-pack-mcp` | The certificate's time has run out. |
| `unknown_variant` | `certificate_name_mismatch` | `a2m-auth`, `a2m-pack-http`, `a2m-pack-mcp` | The certificate is for another name than the address. |
| `unknown_variant` | `certificate_not_verified` | `a2m-auth`, `a2m-pack-http`, `a2m-pack-mcp` | The certificate was refused for another reason, or the operating system's verifier refused it without saying which. |
| `unknown_variant` | `certificate_untrusted` | `a2m-auth`, `a2m-pack-http`, `a2m-pack-mcp` | The certificate the system showed does not lead to an authority this machine trusts. |
| `unknown_variant` | `commit_skipped` | `a2m-engine` | A binding ended without sending its write. |
| `unknown_variant` | `condition_not_boolean` | `a2m-engine` | A condition gave a value that is neither true nor false. |
| `unknown_variant` | `device_identity_mismatch` | `a2m-pack-device` | The device's model, firmware version, or serial is not the one the map and the surface name. |
| `unknown_variant` | `document_type_refused` | `a2m-pack-http` | An XML answer that declares a document type. None is read, so no entity it defines is expanded and nothing it names is fetched (decision 0217). |
| `unknown_variant` | `driver_panicked` | `a2m-engine` | A provider's code panicked. The engine goes on; the step does not. |
| `unknown_variant` | `expect_failed` | `a2m-engine` | An `expect` does not hold. |
| `unknown_variant` | `field_not_read_back` | `a2m-engine` | Reading a record gave no value for a field the operation writes. |
| `unknown_variant` | `field_read_only` | `a2m-pack-desktop` | A field takes no value today: it is read-only, or not editable. Nothing was set. |
| `unknown_variant` | `finding_not_declared` | `a2m-provider` | A discoverer made a finding of a kind or level it did not declare. |
| `unknown_variant` | `fixture_no_answer` | `a2m-provider` | An offline test's fixture records no answer for a step. |
| `unknown_variant` | `formula_at_place` | `a2m-pack-file` | A formula where a value was declared. |
| `unknown_variant` | `handshake_failed` | `a2m-provider` | A subsystem failed its handshake. |
| `unknown_variant` | `ledger_state_unexpected` | `a2m-ledger` | A transition no rule allows. |
| `unknown_variant` | `metadata_not_kept` | `a2m-pack-file` | The old file's permissions cannot be carried to the new file. |
| `unknown_variant` | `net_answer_unreadable` | `a2m-netstate` | The system's answer has a shape this reader does not know. |
| `unknown_variant` | `net_reader_not_built` | `a2m-netstate` | No reader is built for this system. |
| `unknown_variant` | `no_read_back` | `a2m-engine` | A binding has no steps that read the record, so nothing it writes can be verified. |
| `unknown_variant` | `output_unexpected` | `a2m-engine` | A step returned a value the plan gives no place to. |
| `unknown_variant` | `page_unreadable` | `a2m-pack-web-probe` | A page could not be read, in a way no rule covers. |
| `unknown_variant` | `physical_not_built` | `a2m-pack-device` | A step writes a register with the hazard `physical`, and such a write is not built. Nothing was sent. |
| `unknown_variant` | `read_too_large` | `a2m-pack-desktop`, `a2m-pack-web-ui` | A value is longer than the pack reads. |
| `unknown_variant` | `reconcile_too_late` | `a2m-provider` | A committing step returned values for reconcile, which is too late. |
| `unknown_variant` | `response_too_large` | `a2m-pack-http` | An answer larger than the limit. |
| `unknown_variant` | `result_too_large` | `a2m-pack-data` | A statement's result holds more text than the limit allows. |
| `unknown_variant` | `runner_protocol_unsupported` | `a2m-sched` | A runner speaks a protocol version this scheduler does not. |
| `unknown_variant` | `session_move_illegal` | `a2m-auth` | The keeper was asked for a move that is not in the table of states. It is a fault of the keeper itself, and the move is not made. |
| `unknown_variant` | `shape_unsupported` | `a2m-pack-file` | A shape of file, place, or value the pack has no rule for. |
| `unknown_variant` | `step_kind_unknown` | `a2m-provider` | A step of a kind its mode did not declare. |
| `unknown_variant` | `table_not_owned` | `a2m-ledger` | A statement named a table its crate does not own. |
| `unknown_variant` | `tls_handshake_failed` | `a2m-auth`, `a2m-pack-http`, `a2m-pack-mcp` | The TLS handshake failed for a reason that is not the certificate: no version or cipher in common, or the far end ended it. |
| `unknown_variant` | `undeclared_code` | `a2m-provider` | A driver returned a failure code nothing declares. |
| `unknown_variant` | `unexpected_absent` | `a2m-provider` | A step that is no lookup reported its record absent. |
| `unknown_variant` | `unexpected_control` | `a2m-pack-desktop`, `a2m-pack-web-ui` | The element does not offer what the step needs: an editable text, or an action of the name the screen declares. |
| `unknown_variant` | `unexpected_dialog` | `a2m-pack-web-ui` | A dialog of the page that no screen declares. |
| `unknown_variant` | `unexpected_exit_status` | `a2m-pack-command` | A read ended with a status nothing declares, or by a signal. |
| `unknown_variant` | `unexpected_page` | `a2m-pack-desktop`, `a2m-pack-web-ui` | The window a screen declares did not appear, or did not become ready, by the step's deadline. |
| `unknown_variant` | `unexpected_prompt` | `a2m-pack-command` | A read's output holds a declared prompt. |
| `unknown_variant` | `unexpected_reply` | `a2m-pack-device`, `a2m-pack-stream` | The device answered a read with a reply of no declared form. |
| `unknown_variant` | `unexpected_response` | `a2m-auth` | The identity service answered with something that is not a token response. |
| `unknown_variant` | `unexpected_status` | `a2m-pack-data`, `a2m-pack-http`, `a2m-pack-mcp`, `a2m-pack-web-ui` | The store answered with an error no rule covers. |
| `unknown_variant` | `unknown_control` | `a2m-pack-web-ui` | The plan names a control this driver has no rule for. |
| `unknown_variant` | `value_kind_unsupported` | `a2m-pack-data` | A stored value is a float or bytes, which no rule reads. |
| `unknown_variant` | `value_not_held` | `a2m-pack-desktop` | After a `set`, the field holds another value than the one set: the application changed it. Nothing was saved. |
| `unknown_variant` | `value_unreadable` | `a2m-pack-file` | A place holds something its field type cannot read. |
| `unknown_variant` | `watch_key_repeated` | `a2m-ledger` | A record's key appears twice in one check: a named fault of that check, never a merge. |
| `unknown_variant` | `watch_shapes_mixed` | `a2m-ledger` | A check holds keyed records and a whole result at once, or more than one whole result. |
| `rejected` | `app_validation_error` | `a2m-pack-command`, `a2m-pack-desktop`, `a2m-pack-http`, `a2m-pack-mcp`, `a2m-pack-web-ui` | An exit declared `rejected`. |
| `rejected` | `broker_refused` | `a2m-pack-stream` | The broker refused the message, with its reason. |
| `rejected` | `constraint_violated` | `a2m-pack-data` | A unique, check, not-null, or foreign-key constraint refused a write. |
| `rejected` | `device_refused` | `a2m-pack-device` | The device answered with an exception: an address or a value it will not take. |
| `rejected` | `run_not_completed` | `a2m-actions` |  |
| `mismatch` | `read_back_differs` | `a2m-engine`, `a2m-ledger` | The value read back differs from the value intended. |
| `possible_commit` | `ambiguous_match` | `a2m-pack-command`, `a2m-pack-mcp` | A lookup that must match one record matched more than one. |
| `possible_commit` | `answer_lost` | `a2m-local`, `a2m-mcp` | The engine may have had a request, and its answer was lost: what the request asked may have been done, a write among it. The same code, class, and meaning as the stdio proxy's. |
| `possible_commit` | `cannot_reconcile` | `a2m-engine` | A record in doubt cannot be read, so a person must settle it. |
| `possible_commit` | `no_confirmation` | `a2m-engine`, `a2m-provider`, `a2m-pack-data`, `a2m-pack-device`, `a2m-pack-stream` | A write was sent and the system's answer is not known. |
| `possible_commit` | `reconcile_required` | `a2m-ledger` | A record of the run is still in doubt from an earlier lease and has not been read back. |
| `possible_commit` | `runner_lost` | `a2m-ledger` | A call carries a lease number other than the run's. |
| `stale` | `binding_changed` | `a2m-engine` | The binding a test or a trial named is no longer the one at its place: it moved, changed its mode, or changed what it does. |
| `stale` | `changed_since_preview` | `a2m-engine`, `a2m-ledger`, `a2m-pack-data`, `a2m-pack-file` | A record no longer holds what the preview saw. |
| `stale` | `drafts_changed` | `a2m-actions` | The drafts of a publish a person was shown changed before they pressed: they compile to another plan. |
| `stale` | `record_absent` | `a2m-pack-command` | A write's exit says the record it names is not there. |
| `stale` | `version_conflict` | `a2m-pack-command`, `a2m-pack-data`, `a2m-pack-http` | An exit declared `stale`. |
| `stale` | `version_not_read` | `a2m-pack-http` | A write that sends a record's version as a precondition, in a pass that read no version for the record. Nothing is sent. |
| `transient` | `connection_lost` | `a2m-local`, `a2m-provider`, `a2m-pack-data`, `a2m-pack-device`, `a2m-pack-mcp`, `a2m-pack-stream`, `a2m-pack-web-browser` | The connection to the engine ended before a request was sent, so the engine never had it. |
| `transient` | `device_busy` | `a2m-pack-device` | The device answered that it is busy. Nothing was carried out. |
| `transient` | `engine_fault` | `a2m-api` | The dispatcher stopped without an answer. 500. |
| `transient` | `frame_corrupt` | `a2m-pack-device` | A reply was damaged, on a read. |
| `transient` | `net_read_failed` | `a2m-netstate` | The system's call failed. |
| `transient` | `nothing_arrived` | `a2m-pack-stream` | No message matched within the limit, and the operation needs one. |
| `transient` | `page_not_loaded` | `a2m-pack-web-probe` | A page did not load. |
| `transient` | `prepare_timed_out` | `a2m-engine` | A driver was not ready within the time preparing may take. |
| `transient` | `stdio_read_failed` | `a2m-mcp` | The proxy could not read its input. |
| `transient` | `timeout` | `a2m-provider`, `a2m-pack-data`, `a2m-pack-device`, `a2m-pack-mcp`, `a2m-pack-stream`, `a2m-pack-web-browser` | A deadline passed on something that commits nothing. |
| `transient` | `upstream_unavailable` | `a2m-auth`, `a2m-pack-command`, `a2m-pack-data`, `a2m-pack-http`, `a2m-pack-mcp`, `a2m-pack-stream` | The identity service did not answer, or answered that it cannot serve. |
| `busy` | `accessibility_off` | `a2m-pack-desktop` | The session's accessibility switch is off and the application publishes no tree (Linux), or the person has not allowed this program to use the system's accessibility interface (macOS). The pack changes no setting. |
| `busy` | `acknowledgement_not_kept` | `a2m-engine` | The control plane cannot end a record on the system's acknowledgement alone; the record stays `acted`, in doubt. |
| `busy` | `application_not_running` | `a2m-pack-desktop` | No running application is the program and the name the surface states. The pack starts nothing. |
| `busy` | `blocked_by_parent` | `a2m-engine` | A record waits on a parent record that failed. |
| `busy` | `browser_unavailable` | `a2m-pack-web-browser`, `a2m-pack-web-ui` | No browser program was found. |
| `busy` | `caller_queue_full` | `a2m-sched` | The caller has as many jobs waiting as it may. |
| `busy` | `capture_not_kept` | `a2m-engine` | The control plane keeps no captures; the capture was dropped. |
| `busy` | `channel_unavailable` | `a2m-local` | The channel, a lock file, or the loopback listener cannot be opened on this machine. |
| `busy` | `cleanup_incomplete` | `a2m-provider` | Leftovers of an earlier stop block a new start. |
| `busy` | `control_unanswered` | `a2m-engine` | A call to the control plane was not answered within the time the engine waits for one. |
| `busy` | `data_directory_gone` | `a2m-local` | The data directory is no longer the one the engine opened: its engine lock is gone, or another file stands in its place. The engine refuses the request and stops; the next client starts a new engine. |
| `busy` | `disk_full` | `a2m-provider` | The disk, or the user's share of it, is full. |
| `busy` | `engine_degraded` | `a2m-engine`, `a2m-sched` | A write was asked of a kernel that takes reads only. |
| `busy` | `engine_running` | `a2m-local` | An engine already holds the engine lock. |
| `busy` | `engine_unreachable` | `a2m-local` | Nothing answers on the local channel. |
| `busy` | `evidence_not_stored` | `a2m-engine` | Evidence could not be stored, so nothing can be marked verified. |
| `busy` | `file_locked` | `a2m-pack-file` | The lock signal is present, or the system refuses to share the file. |
| `busy` | `job_kind_not_built` | `a2m-engine` | The job is of a kind this engine does not carry out yet. |
| `busy` | `key_record_conflict` | `a2m-vault` | A key record already exists, or changed before it could be replaced. |
| `busy` | `key_store_entry_missing` | `a2m-vault` | The root that wrapped a key is no longer in the key store. |
| `busy` | `key_store_unreachable` | `a2m-vault` | The key store does not answer. The kernel is degraded until it does. |
| `busy` | `lease_too_short` | `a2m-actions` | A step may take longer than a job's lease lasts. |
| `busy` | `message_may_be_held` | `a2m-pack-stream` | A message delivered again may be held by the record that took it first; nothing was taken. |
| `busy` | `no_binding` | `a2m-engine` | No binding qualifies, for a reason that is not a session, an edge block, or drift. |
| `busy` | `no_rate_budget` | `a2m-engine` | No rate budget was granted for a surface. |
| `busy` | `on_hold` | `a2m-sched` | The work is covered by a hold, such as the stop-everything switch. |
| `busy` | `path_locked` | `a2m-provider` | Another program holds the file, or a lock on it. |
| `busy` | `program_busy` | `a2m-pack-command` | An exit declared `busy`, or another start holds the surface. |
| `busy` | `prompt_busy` | `a2m-local` | A presence prompt is already up on this machine, and one is raised at a time. |
| `busy` | `provider_unavailable` | `a2m-provider` | The provider is not ready here, now. |
| `busy` | `queue_full` | `a2m-provider`, `a2m-sched` | A subsystem's queue is full. |
| `busy` | `randomness_unavailable` | `a2m-vault` | The operating system gave no random bytes, so no key or nonce was made. |
| `busy` | `rate_limited` | `a2m-sched` | A rate budget is spent and does not refill inside the lease. |
| `busy` | `record_locked` | `a2m-ledger` | Another run holds the lock on the record. |
| `busy` | `request_timed_out` | `a2m-api` | The dispatcher did not answer within the time allowed. The action may still finish. 503. |
| `busy` | `resource_in_use` | `a2m-sched` | A wait for an exclusive resource ran out. |
| `busy` | `resource_order` | `a2m-sched` | A job asked for an exclusive resource that sorts before one it holds. |
| `busy` | `run_call_unreadable` | `a2m-actions` | What a run was called with is missing from the store or cannot be read. |
| `busy` | `run_reconciling` | `a2m-sched` | A resume found the run's reconcile as its one job: what is in doubt is settled first. |
| `busy` | `run_time_exceeded` | `a2m-engine` | The run passed the longest time its pipeline allows. |
| `busy` | `runner_unavailable` | `a2m-sched` | No runner can serve the job. |
| `busy` | `session_in_use` | `a2m-sched`, `a2m-pack-desktop` | A wait for a place on a session ran out. |
| `busy` | `shadow_not_kept` | `a2m-engine` | The control plane keeps no shadow comparisons, so no binding is run in shadow (decision 0207). |
| `busy` | `shadow_time_exceeded` | `a2m-engine` | A shadow binding did not end within the time a shadow may take; it was cancelled and counted as not compared. |
| `busy` | `shadow_write_not_built` | `a2m-ledger` | The shadow of a binding of an operation that writes is not built: its comparison cannot be stored, and it is never promotable through shadow. |
| `busy` | `shutting_down` | `a2m-provider` | The supervisor is stopping and starts nothing. |
| `busy` | `start_failed` | `a2m-provider` | A program could not be started by the operating system. |
| `busy` | `storage_full` | `a2m-ledger`, `a2m-telemetry`, `a2m-pack-file` | A blob or a row cannot be written for lack of space. |
| `busy` | `store_corrupt` | `a2m-ledger` | The integrity check, a stored row, a stored plan's hash, or the audit chain fails. |
| `busy` | `store_locked` | `a2m-pack-data` | Another connection held the store's lock for the whole time limit. |
| `busy` | `store_unavailable` | `a2m-ledger` | The database cannot commit, or the store is not ready. |
| `busy` | `store_version_unsupported` | `a2m-ledger` | The store was written by a newer engine, or a migration it applied is not the one this engine carries. |
| `busy` | `subscription_in_use` | `a2m-pack-stream` | The runtime's own subscription is held by another run. |
| `busy` | `subsystem_limit` | `a2m-provider` | As many subsystems of this kind run as may, or the place is taken. |
| `busy` | `surface_unavailable` | `a2m-local` | The person surface cannot show a page: it holds no approval desk. |
| `busy` | `system_job_not_carried` | `a2m-actions` | The in-process runner loop was handed a job that is not a run's. |
| `busy` | `system_not_built` | `a2m-pack-desktop` | This pack has no back end for the operating system it runs on. |
| `busy` | `target_elevated` | `a2m-pack-desktop` | The application runs at a higher level than the engine, as administrator. Windows only; the pack does not reach it. |
| `busy` | `tool_changed` | `a2m-actions` | A published tool changed between the decision about a call and the call. |
| `busy` | `watch_check_running` | `a2m-ledger` | A check of the watch is under way already. |
| `busy` | `watch_check_unfinished` | `a2m-ledger` | A check's run has not reached an end the watch can take. |
| `busy` | `watch_due_missed` | `a2m-ledger` | Due times passed with no check: the engine was not running, the watch was paused or held, or a check could not be admitted in time. The code of a `gap`, |
| `busy` | `watch_over_budget_share` | `a2m-sched` | The watches on one surface would take more of its budget than their share. |
| `busy` | `watch_paused` | `a2m-ledger` | The watch is paused. |
| `busy` | `watch_too_many` | `a2m-sched` | The person has as many watches as they may have. |
| `busy` | `writes_not_built` | `a2m-pack-desktop` | This pack writes only on Linux in this version. Nothing was sent. |
<!-- failure-codes:end -->

### Meanings not yet stated in the code

Names the hand-kept list held before the table was generated, which no crate declares today. Each is kept here, by hand, so that nothing a reader had is gone; a name that a crate begins to declare moves into the generated table, and this row is then removed. The earlier list gave no meaning for any code, so there is none to carry over.

| Class named by the earlier list | Code |
| --- | --- |
| `edge_blocked` | `challenge_page` |
| `input` | `draft_under_release_version` |

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
| What a command adds | One request header, read from the local channel alone: `A2M-Project`, the project folder it was run in, percent-encoded. No header says a person is at the caller's terminal: every caller of the channel is an agent ([decision 0158](../decisions/0158-the-product-crate-and-the-local-engine-after-their-safety-audit.md)). Every REST answer carries its exit class in the header `A2M-Exit-Class` ([decision 0131](../decisions/0131-the-front-doors-joined-serve-the-thin-client-and-the-bridge.md)) |
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
