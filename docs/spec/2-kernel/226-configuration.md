# 226. Configuration

Anything2MCP is meant to be fitted closely to where it runs: one person's laptop, a job in continuous integration, a company server. This section is the whole design of what can be configured and how: the principles, the guardrails no setting can reach, the layers and their precedence, the scopes at which something is set, the protected settings, the presets, every group of settings, the settings each pack brings, what is configured in a target's files instead, and how a change takes effect. The settings types and the loader that layers them live in `a2m-actions`, so that `config.show` and `config.set` speak the same rules as start-up ([crate design](../../architecture/crates/a2m-actions.md)). Settings are TOML, target files are YAML ([decision 0014](../../decisions/0014-declarative-files-yaml-targets-toml-settings-cel-expressions.md)).

Key names in this section are a draft until the settings loader is built. The groups, the defaults, the scopes, and what is protected are the design.

## Principles

1. **Every setting has a default, and the defaults together are the safe single-user installation.** An empty file, or no file, is a complete configuration: the Solo profile, one engine per user, writes approved by a person every run, nothing sent anywhere.
2. **Every setting is typed, documented, and validated at start.** A value of the wrong type, out of range, or naming something that does not exist stops the load with the file, the line, the key, and the fix. **An unknown key is an error, never ignored**, with the nearest known keys offered, so a misspelt safety setting can never be silently without effect.
3. **One schema.** Each setting is declared once: its key, type, allowed values, default for each profile, scope, whether it is protected, which direction is safer, when a change takes effect, and its description. The documentation that `docs.get` serves, the validation at start, the editor schema, and the output of `config.show` are all generated from it. A test fails if a key is read anywhere that the schema does not declare.
4. **A change is audited.** `config.set` writes an audit entry: who, which key, the old and new values, the layer, and the presence check passed. At every start the loader records the digest of the effective settings, and, where it differs from the last start, the keys that changed and the layer each came from.
5. **Configuration is data that can be kept in version control.** Files hold no secret. A value such as a database password is a reference, `vault:<name>` or `env:<NAME>`, resolved when it is used and registered with the redactor ([section 216](216-the-vault-and-its-keys.md)). A settings file can be reviewed and committed like a target file.
6. **Nothing phones home unless switched on.** Every setting that would send anything beyond the targets, the identity provider, and a configured control plane is off by default, protected, and says exactly what it sends ([decision 0040](../../decisions/0040-nothing-leaves-the-machine-unless-switched-on.md), [section 605](../6-running-it/605-packaging-and-release.md)).
7. **Settings only narrow what a target declares.** A setting is a ceiling, a floor, or a choice of how. It can lower a target's rate budget or time limit; it can never raise one, and never widen where a request may go.

## What no setting can do

Configurability stops at safety. The rules below are compiled into the binary. Most have no key in the schema at all, so a file that tries to set one fails as an unknown key. Where a key sits next to one, its range excludes the loosening, and a value that would weaken a guardrail is refused when the file loads, with the guardrail named. The policy guardrails among them are forbids compiled with their own hash and added to every policy set ([section 212](212-policy-and-approvals.md)). No profile, preset, project file, environment variable, flag, or per-call option reaches them.

| # | No configuration can | Held by |
| --- | --- | --- |
| 1 | Put a model, or any improvising step, into a run | There is no such key. A run carries out a compiled plan ([section 207](207-the-engine-loop.md)) |
| 2 | Let a write go ahead without an intent recorded first, or count it done without a read-back compared after normalization | The engine loop and the ledger's rules ([section 208](208-runs-the-ledger-and-resume.md)). A write that cannot be read back is `irreversible` |
| 3 | Let a write run with no ledger, or let anything run when the store or policy cannot be trusted | Fail closed ([section 224](224-when-the-engine-itself-fails.md)) |
| 4 | Let a secret reach a caller, a log, a trace, a capture, or a fixture | The redactor cannot be switched off, and cookie and authorization values are dropped at every capture level ([section 217](217-traces-evidence-and-repair.md)). Redaction rules can be added, never removed |
| 5 | Let discovery or the scout write, try a write, or skip consent | The engine's sandbox, where no intent token exists ([section 218](218-discovery-and-findings.md), [section 402](../4-building-tools/402-the-scout-and-who-does-what.md)) |
| 6 | Let an agent approve a run or a publish, or let a caller's own confirmation stand where policy names an approver | Built-in forbids; no action approves ([section 220](220-actions-and-the-authoring-interface.md)) |
| 7 | Run an `irreversible` operation without a person, or under a standing approval; pre-approve any hazard other than `none`; give `device_function` or `physical` a standing approval; retry `sends_outside` | [Section 213](213-effects-hazards-and-consent.md) |
| 8 | Let the author of a version be its only publisher | Built-in forbid |
| 9 | Let a caller name the identity it runs as, or use another person's session, whatever its role | Built-in forbid ([section 215](215-sessions-and-the-session-keeper.md)) |
| 10 | Let a run reach a surface or destination its operation did not declare, or let an input choose a host, a path, a program, or a file | The permission manifest and typed templates ([section 502](../5-security/502-defending-against-a-manipulated-agent.md)) |
| 11 | Get past a protection: a sign-in, a second factor, a bot defence, a CAPTCHA, a licence check, a device's protection; hide that the engine is automation; spoof a fingerprint or imitate another application's client | There is no such key. A block is a named stop |
| 12 | Point the engine at a person's everyday browser profile or another program's sign-in store | The web pack accepts only a profile directory the engine created ([decision 0031](../../decisions/0031-the-browser-always-runs-with-a-profile-the-engine-owns.md)) |
| 13 | Remove or bypass a kill switch | Checked first on every decision |
| 14 | Turn a named stop into a guess: a fallback between providers, a best-effort mode, a retry of a class whose rule forbids one | The failure classes decide what happens next ([section 207](207-the-engine-loop.md)) |
| 15 | Run what was not published, or use a faster binding before it passed shadow and a person promoted it | The registry of hashes ([section 206](206-the-compiler-and-the-plan.md)) |
| 16 | Turn a command into a shell, or accept a statement built from an input | The command and data packs ([section 306](../3-packs/306-the-command-pack.md), [section 307](../3-packs/307-the-data-pack.md)) |
| 17 | Widen policy from a lower layer | Policy only narrows ([section 212](212-policy-and-approvals.md)) |

## Layers and precedence

Settings come from layers. A higher layer overrides a lower one, key by key, lowest first:

| # | Layer | Where | Typical use |
| --- | --- | --- | --- |
| 1 | Built-in defaults | The binary | The safe single-user installation |
| 2 | The deployment profile | `profile.name`, from any file layer | Defaults that differ by shape, such as the scout running on add only in `solo` and `workstation` |
| 3 | A preset | `profile.preset`, from any file layer | Twenty values set by one choice |
| 4 | The installation's settings file | The installation's configuration directory, owned by whoever installed the engine | A server's store, listeners, identity provider, network |
| 5 | The user's settings file | The user's configuration directory | One person's preferences on a shared machine |
| 6 | The project's file, `anything2mcp.toml` | The project folder, with the target files | What a project needs, committed with it |
| 7 | Environment variables, `A2M_<GROUP>__<KEY>` | The process environment | Containers and continuous integration |
| 8 | A command's flags | The command line | One run of one command |

**What a layer may set.** Precedence decides which value wins; it does not let any layer loosen anything.

- **Policy and protected settings.** Layers 5 to 8 may tighten a protected setting, never loosen it. Each protected setting declares its safer direction in the schema; a value on the looser side from those layers is refused with the setting and the layer named. A project file therefore cannot loosen an installation's policy, and neither can an agent that sets an environment variable or passes a flag. How a protected setting is loosened is below.
- **Installation-only settings.** The store, the vault, the profile's roles, the listeners, and trust roots are read from layers 1 to 4 and from the flags of `serve` alone. A user or project file that sets one is an error.
- **Locks.** The installation file may lock any key with `[locks] keys = [...]`, wildcards by group allowed. No higher layer may change a locked key in either direction, and trying is an error naming the lock.
- **Per-call options** are not a layer. A caller may set only the options an operation offers, such as pinning a mode, and only where the policy question "may this caller set this per-call option?" allows it.

**Where each value came from.** `config.show` prints every key with its effective value, the layer that set it (with the file and line, the variable, or the flag), whether it is locked or protected, every lower layer that set it and was overridden, and every value that was refused and why. Secret references are shown as references. With a key, it shows that key's documentation from the schema.

## Scopes

A setting is how this installation behaves. A target file is what a system is: its surfaces, endpoints, screens, operations, limits, and environments, and it travels with the target to every installation that imports it ([section 205](205-target-files.md)). The test: if a value must hold wherever the target runs, it belongs in the target's files; if it depends on the machine, the organization, or the people, it is a setting. Nothing that names a surface, an endpoint, a field, or a locator is a setting (knowledge lives in one place).

| Scope | What is set there | Where it lives | Who may change it | Example |
| --- | --- | --- | --- | --- |
| Installation | Every group below | Layers 1 to 4 | The person or operator who installed it | `store.database` |
| Workspace | Policy, grants, toolsets, quotas, retention within the installation's bounds | The store, as a published policy set and toolsets | A workspace administrator, through `policy.publish`, `grant.add`, `toolset.update` | Writes need a named approver in one workspace |
| Project | Authoring and the scout; tighter limits for this project | `anything2mcp.toml` | The developer | `authoring.scout.run = "on_request"` |
| Target | Surfaces, rates, operations, environments, which bindings exist | The target's files, versioned and published | The target's author, then a publisher | A surface's request budget |
| Environment, test or production | Addresses, declared test records, which writes the gates may try | The target's environments file | The target's author; test records are declared by a person ([section 403](../4-building-tools/403-testing-and-publish-gates.md)) | Writes run in the gates only in `test` |
| Surface | Request budget, how it stands to other surfaces, its identity strategy | The target's files | The target's author | Two requests a second |
| Toolset | Which tools, which version, identity mode, keep-alive window on a server, how the tool list is shaped | The store | Whoever defines the toolset, through `toolset.create` and `toolset.update` | Keep-alive 4 hours for `orders` |
| Operation | Effect class, hazard, time limits, record limit, binding order | The target's files | The target's author | A 30-second limit on one save |
| Caller or grant | Highest effect class, identity mode, scope, quota | The store | An administrator, through `grant.add` | An agent may read `example` only |
| Provider or pack | Enabled or not; the pack's own group | Layers 1 to 8 for the group; `provider.enable` for enabling | The installer; enabling some providers is protected | `packs.web.headless = false` |
| Session | Nothing is set on a session directly. Its keep-alive, probe, and attempt limit come from settings, the toolset, and the target's auth file | Derived | Nobody; `session.revoke` ends one | |
| Run | Per-call options the operation offers and policy allows | The call | The caller, within policy | Pin the `http` mode |

## Protected settings

A protected setting is one whose looser value only a person can choose, through a channel an agent cannot reach. On one machine that channel is `config.set` or `provider.enable` with the operating system's user-presence prompt, the same one an approval uses ([section 503](../5-security/503-approval-and-sign-in-surfaces.md)). The loosened value is kept in the store as a record bound to the presence check, and takes the place of the user layer for that key. A file, a variable, or a flag that loosens it is refused. On a server the channel is the installation file, which only the operator's account can write, and the console after an administrator signs in. Tightening a protected setting is never protected.

| Protected setting | Loosening means | Reference |
| --- | --- | --- |
| `policy.approve_writes` | Lowering who must approve a write | [Section 212](212-policy-and-approvals.md) |
| `policy.max_records_per_run` | Raising the record limit | Section 212 |
| `policy.preview_freshness_seconds`, `policy.approval_window_minutes` | Lengthening either | [Section 208](208-runs-the-ledger-and-resume.md) |
| `files.allowed_roots` | Adding a root | [Section 502](../5-security/502-defending-against-a-manipulated-agent.md) |
| `evidence.capture_on_writes` | Lowering capture on writes | [Section 217](217-traces-evidence-and-repair.md) |
| `evidence.audit_retention_days` | Shortening it | Section 217 |
| `authoring.capture.values` | Keeping captured values, not only shapes | [Section 401](../4-building-tools/401-overlaying-tools-on-an-existing-system.md) |
| `providers.enabled` | Enabling a provider that starts a process or reaches a network, or whose effects are only `declared` | [Section 203](203-the-provider-contract.md), [section 213](213-effects-hazards-and-consent.md) |
| `plugins.allow_unsigned` | Allowing an unsigned plug-in, by digest | [Section 202](202-kernel-and-packs.md) |
| `providers.attach` | Attaching to a process the runtime did not start | Section 213 |
| `engine.shadow.matches_to_promote` | Lowering the matches a binding needs | [Section 206](206-the-compiler-and-the-plan.md) |
| `network.allowed_hosts`, `network.trust_roots`, `network.proxy` | Adding a destination, a certificate authority, or a proxy | [Section 603](../6-running-it/603-networking.md) |
| `server.local_http` | Opening a local port on a workstation | [Section 222](222-front-doors-and-the-local-engine.md) |
| `telemetry.export`, `telemetry.crash_reports`, `updates.check`, `server.webhooks`, `alerts.destinations` | Sending anything off the machine | [Section 605](../6-running-it/605-packaging-and-release.md) |

Releasing a kill switch is not a setting, but on one machine `killswitch.set` that releases one needs the same presence prompt. Running code that came from a target, and opening a link to a device, are consent records a person makes for one target, never settings ([section 213](213-effects-hazards-and-consent.md)).

## Presets

A preset is a named bundle of values that a person picks instead of setting twenty. It is data: a TOML file of ordinary keys, checked by the same schema. It is layer 3, so the installation, user, and project files still override it, and `config.show` names the preset as the source of every value it set. `config.show` lists the presets and shows what each one sets.

| Preset | For | What it sets |
| --- | --- | --- |
| `standard` | The default; one person, at the keyboard | Nothing. It is the defaults |
| `cautious` | A first look at a system, or a person who wants every step in view | Grants top out at `read` for agents; writes, even pre-approved ones, need a person each run; `authoring.scout.run = "on_request"` and `try_reads = false`; `evidence.capture = "standard"` for reads too; record limit 10; keep-alive 1 hour |
| `ci` | A job in continuous integration, with no person present | `engine.in_process = true`; `engine.person = "absent"`, so anything that would wait for a person fails at once; writes only to declared test records in a `test` environment, through `test.run`; the scout and capture off; logs as JSON to standard output; `sessions.login_wait_minutes = 0` |

**Rules for presets.**

- A preset may tighten a protected setting relative to the defaults, never loosen one. The built-in presets only tighten, so choosing one needs no presence check.
- A site defines its own preset as a file in the installation's `presets` folder. It is listed and inspected like the built-in ones, and refused at load if it loosens a protected setting. A site that wants a looser value sets it in the installation file, where it is visible as such.
- One preset at a time. A preset cannot name another.

## The groups

Each table gives the key, its type or values, its default on one machine, its scope (I installation only; U user and above; P project and above), and whether it is protected (yes, or no). Defaults that differ by profile say so.

**Profile and roles.**

| Key | Values | Default | Scope | Protected |
| --- | --- | --- | --- | --- |
| `profile.name` | `solo`, `workstation`, `team`, `company`, `hybrid`, `cluster`, `embedded` ([section 601](../6-running-it/601-deployment-configurations.md)) | `solo` | I | no |
| `profile.roles` | `control_plane`, `runner`, both | Both | I | no |
| `profile.preset` | A built-in or site preset | `standard` | U | no |

**Storage, the vault, and files.**

| Key | Values | Default | Scope | Protected |
| --- | --- | --- | --- | --- |
| `store.database` | A SQLite path, or a Postgres address with a secret reference | One SQLite file in the data directory | I | no |
| `store.blobs` | A directory, or an object store address | The data directory | I | no |
| `store.free_space_floor` | Bytes | A draft: 1 GiB; below it the kernel is `degraded` ([section 224](224-when-the-engine-itself-fails.md)) | I | no |
| `vault.backend` | `os_keystore`, or an external secrets manager by reference | `os_keystore` | I | no |
| `vault.root_key` | A key reference | The key store's own | I | no |
| `files.allowed_roots` | Named folders | The project folder | U | yes |
| `files.max_upload_bytes` | Bytes | A draft | U | no |

**Policy.** Pre-approval rules, standing approvals, grants, and quotas are in the policy set, not here.

| Key | Values | Default | Scope | Protected |
| --- | --- | --- | --- | --- |
| `policy.approve_writes` | `person_each_run`; on a server also `caller_confirms`, `named_approver` | `person_each_run` | I, U | yes |
| `policy.two_person_irreversible` | Boolean | Off | U | no (raising is tightening) |
| `policy.max_records_per_run` | Count | 50 | U | yes |
| `policy.approval_window_minutes` | Minutes | 15 | U | yes |
| `policy.preview_freshness_seconds` | Seconds | 60 | U | yes |

**Identity and sessions.** On a server, the keep-alive window is set per toolset and this is its default.

| Key | Values | Default | Scope | Protected |
| --- | --- | --- | --- | --- |
| `identity.default_mode`, `identity.allowed_modes` | Identity modes ([section 215](215-sessions-and-the-session-keeper.md)) | The user's own sign-in | I | no |
| `sessions.keep_alive_hours` | Hours after last use | 8 | U | no |
| `sessions.probe_minutes` | Minutes, or `learned` | 15, or half the idle timeout | U | no |
| `sessions.login_wait_minutes` | Minutes an interactive call waits for a sign-in | 5 | U | no |
| `sessions.attempt_limit` | 1 to 3 tries of a stored credential | 1 | I | no |

**The engine.** Retries are fixed by failure class: only `transient` has a count, and only for reads and idempotent steps; `mismatch` re-reads once; every other class never retries.

| Key | Values | Default | Scope | Protected |
| --- | --- | --- | --- | --- |
| `engine.idle_stop_minutes` | Minutes | 30 | U | no |
| `engine.in_process` | Boolean | Off | U | no |
| `engine.person` | `present`: work that needs a person waits; `absent`: it stops at once with its named state | `present` | U | no |
| `engine.retries.transient` | 0 to 5 | 2 | U | no |
| `engine.time_limit_ceiling` | Duration; the longest step time limit a target may declare here | A draft: 10 minutes | I | no |
| `engine.shadow.matches_to_promote` | `{ read, write }` | `{ read = 5, write = 20 }` | I | yes |
| `engine.promoted_writes` | `after_promotion`, `never`: whether writes may use a binding promoted from shadow | `after_promotion` | U | no |
| `ledger.intent_window.<mode>` | 1 up to the manifest's value | The mode's `intent_window` in its manifest | U | no |
| `modes.disabled` | Modes that may not be used here | None | U | no |

The order of bindings is not a setting. It is the operation's, or each mode's rank in its provider's manifest ([section 201](201-archetypes-and-modes.md)). A disabled mode makes its bindings unhealthy, and reads fall to the next verified binding as for any unhealthy one.

**Scheduling.** Defaults per mode come from each mode's manifest ([section 211](211-concurrency.md)).

| Key | Values | Default | Scope | Protected |
| --- | --- | --- | --- | --- |
| `scheduler.<mode>.session_reads`, `scheduler.<mode>.session_writes` | Count, up to the manifest's value | The manifest's | U | no |
| `scheduler.queue_length_per_target` | Count | 100 | U | no |
| `scheduler.priority` | `interactive_first`, `fifo` | `interactive_first` | I | no |
| `runners.lease_seconds`, `runners.heartbeat_seconds`, `runners.quarantine_seconds`, `runners.certificate_hours` | ([section 223](223-the-runner-protocol.md)) | 30, 10, 60, 24 | I | no |

**Authoring and the scout** ([section 402](../4-building-tools/402-the-scout-and-who-does-what.md)).

| Key | Values | Default | Scope | Protected |
| --- | --- | --- | --- | --- |
| `authoring.scout.run` | `on_request`, `on_add` | `on_add` in `solo` and `workstation`; `on_request` otherwise | P | no |
| `authoring.scout.try_reads` | Boolean | On | P | no |
| `authoring.scout.budget` | Time, requests, depth | Small enough that an ordinary system ends within minutes; a target until measured | P | no |
| `authoring.scout.providers` | Optional discoverers by name | None; first-party discoverers are always available | P | no |
| `authoring.capture.values` | Boolean: keep values, not only shapes | Off | P | yes |
| `authoring.capture.limits` | Time, bytes, exchanges | A draft | P | no |

**Evidence and telemetry.**

| Key | Values | Default | Scope | Protected |
| --- | --- | --- | --- | --- |
| `evidence.capture` | `minimal`, `standard`, `full` ([section 217](217-traces-evidence-and-repair.md)) | `minimal` for reads | U | no |
| `evidence.capture_on_writes` | `standard`, `full` | `standard` | U | yes |
| `evidence.trace_retention_days` | Days | 30 | U | no |
| `evidence.audit_retention_days` | Days | 365 | I | yes |
| `evidence.redaction.extra` | Further patterns and field names to mask; added to the built-in rules | None | U | no |
| `telemetry.export` | Off, or an OpenTelemetry endpoint and sampling | Off | I | yes |
| `telemetry.crash_reports` | Boolean | Off | I | yes |

**Front doors** ([section 222](222-front-doors-and-the-local-engine.md), [section 221](221-the-tool-surface-agents-see.md)).

| Key | Values | Default | Scope | Protected |
| --- | --- | --- | --- | --- |
| `server.listen`, `server.tls`, `server.base_url`, `server.trusted_proxies` | Addresses, certificate references | None: no port is opened on one machine | I | no |
| `server.front_doors` | `mcp_http`, `rest`, `console` | None on one machine | I | no |
| `server.local_http` | Boolean, for `workstation` | Off | I | yes |
| `server.webhooks` | Destinations, each checked against egress | None | I | yes |
| `mcp.revisions` | Revisions served | `["2026-07-28", "2025-11-25"]` | I | no |
| `auth.callers.issuer`, `auth.callers.audience`, `auth.callers.api_keys` | The identity provider; API keys on or off | None; API keys off | I | no |
| `tools.listed_limit` | Count of tools listed up front | Fewer than 20 | U | no |
| `tools.full_listing` | Boolean, for clients with their own tool search | Off | U | no |
| `tools.results` | `concise`, `detailed` | `concise` | U | no |
| `tools.result_cap_bytes` | Bytes | A draft | U | no |

**The network** ([section 603](../6-running-it/603-networking.md)).

| Key | Values | Default | Scope | Protected |
| --- | --- | --- | --- | --- |
| `network.egress` | `declared_surfaces`; the identity provider and a configured control plane are always added | `declared_surfaces` | I | no |
| `network.allowed_hosts` | Further hosts, such as a monitoring endpoint | None | I | yes |
| `network.proxy` | An outbound proxy | None | I | yes |
| `network.trust_roots` | Further certificate authorities | The operating system's | I | yes |
| `network.private_addresses` | Refused unless declared, on a server | Refused | I | no |

**Providers, packs, and plug-ins.**

| Key | Values | Default | Scope | Protected |
| --- | --- | --- | --- | --- |
| `providers.enabled` | Provider names; set through `provider.enable` and `provider.disable` | Every first-party pack whose requirements are present; no optional provider | I | yes, for a provider that starts a process, reaches a network, or is only `declared` |
| `providers.mode.<mode>` | The provider that drives a mode two ready providers offer | None, which is `provider_ambiguous` when it matters ([section 203](203-the-provider-contract.md)) | I | no |
| `providers.attach` | Processes a person has named | None | U | yes |
| `plugins.allow_unsigned` | Plug-in digests | None | I | yes |
| `packs.<pack>.*` | The pack's own group, below | The pack's | U | As the pack declares |

**Updates, logging, and locale.**

| Key | Values | Default | Scope | Protected |
| --- | --- | --- | --- | --- |
| `updates.check` | Boolean | Off; on a server updating is off and a version is pinned | I | yes |
| `updates.channel` | `stable`, `beta` | `stable` | I | no |
| `logging.level` | `error`, `warn`, `info`, `debug` | `info` | U | no |
| `logging.format`, `logging.to` | `text`, `json`; the logs directory or standard error | `text`, the logs directory | U | no |
| `locale.messages` | A language for messages and the console | `en` | U | no |
| `locale.display_timezone` | A time zone for times shown to people | The machine's | U | no |

Locale settings change only what people are shown. They never change how a value is parsed or compared: that is the field type's, in the target's files.

## Per-pack configuration

Each pack brings its own group, `packs.<pack>`, under its own name. Its manifest declares its settings, one `[[settings]]` entry for each key ([decision 0089](../../decisions/0089-the-provider-contract-as-building-it-settled-it.md)), under the same rules as the kernel's: every key typed, defaulted, documented, scoped, with protected keys and their safer direction marked. The kernel's loader validates the group against that schema without knowing what it means, generates its documentation and `config.show` output the same way, and hands the pack the validated group through the registry. A group for a pack that is not installed is an unknown key. A pack setting may be a ceiling or a choice of how; it can never widen what a target declares or what the guardrails forbid.

The web pack's group, as the worked example ([section 301](../3-packs/301-the-web-pack.md), [section 302](../3-packs/302-the-web-pack-browser-engine.md)):

| Key | Values | Default | Scope | Protected |
| --- | --- | --- | --- | --- |
| `packs.web.browser` | `installed`, or `pinned` with a version fetched by `browser.install` | `installed` | U | no |
| `packs.web.headless` | Boolean | On; the sign-in window is always visible | U | no |
| `packs.web.profile_dir` | A directory the engine created and owns | Under the data directory, one profile per identity | I | no; a directory holding a profile the engine did not create is refused |
| `packs.web.page_budget` | Open pages for all sessions together | Sized to memory | U | no |
| `packs.web.viewport`, `packs.web.locale`, `packs.web.timezone` | Fixed, for reproducible runs | `[1440, 900]`, `en-US`, `UTC` | U | no |
| `packs.web.recycle_after_pages` | Count before a browser is restarted | A draft | U | no |

What the other packs would configure, from their sections:

| Pack | Its group would hold |
| --- | --- |
| HTTP ([section 303](../3-packs/303-the-http-pack.md)) | Connections kept per surface, the ceiling on a request's time limit, the largest response body read |
| MCP ([section 304](../3-packs/304-the-mcp-pack.md)) | The ceiling on an upstream call's time limit, the largest result accepted, the revisions it will speak upstream |
| Desktop ([section 305](../3-packs/305-the-desktop-pack.md)) | Whether desktop runs may happen on this machine at all, which is off on a server, and the time allowed to find a window |
| Command ([section 306](../3-packs/306-the-command-pack.md)) | Ceilings over a command's declared `limits`: time, grace, and bytes per stream; where the engine-owned state folders live |
| Data ([section 307](../3-packs/307-the-data-pack.md)) | Ceilings over a statement's `time_limit` and row limit; connections kept per surface |
| File ([section 308](../3-packs/308-the-file-pack.md)) | The largest file opened; the largest number of records read at once |
| Stream ([section 309](../3-packs/309-the-stream-pack.md)) | The ceiling on every wait, which a channel's `max` may not pass; the size of a discovery sample |
| Device ([section 310](../3-packs/310-the-device-pack.md)) | Whether serial ports may be listed at all; a floor under every link's `min_gap` and a ceiling on frames per run; the firmware inspection provider, off |

## Configuring a target

What a system is, and how it must be treated, is configured in its target files, not in settings: versioned, reviewed, published, and bundled like the rest, so that it is the same in every profile ([section 205](205-target-files.md)).

| What | Where in the target |
| --- | --- |
| Request budgets | Per surface, from the system's documented limit or a conservative default |
| Environments | `test` and `production`, with their addresses and declared test records |
| Which bindings an operation has, and their order | The operation; a new binding becomes usable only through shadow and `binding.promote` |
| Time limits | Per step and per operation, within the installation's ceilings |
| Record limits | Per pipeline, within the installation's and the toolset's |
| Effect class and hazard | Derived by the compiler from the files |
| Sign-in, the probe, the idle timeout | The auth file |

A ceiling is checked twice. The compiler refuses a value above the ceilings of the installation where it compiles; importing a version into an installation with lower ceilings checks it again, and an operation above them is refused with a named failure, never cut silently. At run time the lower of the target's value and the setting applies.

## Changing configuration at run time

| Takes effect | Settings |
| --- | --- |
| At once | Logging, scheduling limits, queue lengths, keep-alive windows, probe intervals, retries, capture levels, tool-list shaping, retention, presets that change only these |
| At the next start of the engine | The profile and roles, the store, the vault, listeners and front doors, MCP revisions, the network, providers enabled and pack groups, updates |
| With a new published version | Anything in a target's files, a policy set, or a toolset |

Tightening applies to waiting runs at their next check, as a new policy version does. Loosening applies only to runs that start after it.

- `config.show` prints the effective settings and where each came from, as above.
- `config.set` changes one key in a named file layer, the user's by default, after validating it against the schema. On a protected setting it raises the presence prompt first, and it says when the change takes effect.
- `provider.enable` and `provider.disable` change `providers.enabled`, with the presence prompt where the provider is protected.
- `doctor.run` loads the settings as start-up would and reports every problem, each setting left at a value the schema marks as worth a look, which presence check this machine offers, and whether the browser, the key store, the clock, and the configured network settings are usable. It makes no outside connection.

## Worked examples

The empty configuration. With no file at all, the engine is the Solo profile with the `standard` preset:

```toml
```

A cautious single user, who wants every step in view and a second folder for sources:

```toml
[profile]
preset = "cautious"

[files]
allowed_roots = ["<project>", "~/example-sources"]   # protected: needs the presence prompt through config.set

[logging]
level = "debug"
```

A job in continuous integration, run against a test stand:

```toml
[profile]
preset = "ci"

[store]
database = "./ci-store.db"

[evidence]
trace_retention_days = 1
```

A small team server: control plane and runner in one process, on one machine, with the organization's sign-in.

```toml
[profile]
name = "team"

[server]
listen = "0.0.0.0:8443"
base_url = "https://mcp.example.com"
front_doors = ["mcp_http", "rest", "console"]
tls = { certificate = "vault:tls-cert", key = "vault:tls-key" }

[auth.callers]
issuer = "https://login.example.com/tenant-id/v2.0"
audience = "api://anything2mcp"

[store]
database = "postgres://anything2mcp@db.example.com/anything2mcp?password=vault:db-password"

[identity]
default_mode = "per_user_session"
allowed_modes = ["per_user_session", "service_account"]

[policy]
approve_writes = "named_approver"

[locks]
keys = ["policy.*", "network.*", "files.*"]
```

## Open points

- Key names, the form of environment variable names, and the paths of the installation and user files are drafts until the settings loader is built. `config.show` is planned for Phase 1 and `config.set` for Phase 3.
- Defaults marked "a draft" (the free-space floor, the time-limit ceiling, upload and result caps, capture limits, browser recycling) are set when they are first measured.
- Settings at the scope of a workspace beyond policy and toolsets, such as a workspace's trace retention, have no action yet. `config.set` is for the local user.
- How the console on a server records a change to a protected setting, and whether a server's installation file should be signed.
- Whether presets should compose, or stay one at a time.
- Which settings can safely take effect at once on a server with several control-plane replicas, and how the change reaches each.
- How a plug-in's settings schema is read: as the file its manifest names, at build time, like its other schemas.
