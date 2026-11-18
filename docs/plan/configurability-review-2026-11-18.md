# Configurability review, 2026-11-18

One design review round, by a new lens, [lens 16](design-review-lenses.md), the person fitting it, which the owner asked for through the lead. The question, in this review's words: along which whole dimensions would a person, a developer, or an organization expect to shape the product and find they cannot? Not one more setting: the axes. And, as plainly, the other side: what is deliberately not configurable, and of what kind each such line is, so that the fixed frame and the free space inside it can be seen on one page.

The round wrote documents only. It read the settings schema and loader in `a2m-actions`, every reader of a setting in the crates, the packs' manifests, the policy engine's sets and guardrails, the target file formats, and [section 226](../spec/2-kernel/226-configuration.md), and it ran `anything2mcp config --help` and the help of its two commands. It started no engine and wrote no settings.

Its records are [0236](../decisions/0236-a-declared-setting-whose-feature-is-not-built-is-refused-by-name.md) (a declared key nothing reads), [0237](../decisions/0237-one-statement-of-what-is-said-at-which-scope-by-which-means-and-by-whom.md) (the scoping model), and two questions for the owner, [0238](../decisions/0238-whether-a-person-may-configure-one-client-connection-apart-from-another.md) and [0239](../decisions/0239-whether-a-persons-own-code-may-run-on-the-path-of-a-run.md).

## The findings that matter most

| # | Finding | Where | Serious | Kind | Sorted to |
| --- | --- | --- | --- | --- | --- |
| 1 | 55 of the 95 kernel keys are read by nothing. 20 of those govern a part that runs today, so a value a person writes is accepted and has no effect: the specification's promise that no setting is silently without effect does not hold. Only `network.proxy` is refused by name ([decision 0196](../decisions/0196-the-engine-and-the-registry-after-the-acknowledgement-and-the-first-hour.md)) | `settings/schema.rs`, every crate | High | Contradiction | Record 0236 |
| 2 | The project layer is the project the engine started in. A request from another project has its `anything2mcp.toml` checked to parse and never applied, and nothing says so. Variables differ with a `settings_differ` warning; a project file differs in silence | `anything2mcp/src/engine.rs` `sources`, `a2m-actions/src/project.rs` `check_plain`, `cli/start.rs` `plan_for` | High | Contradiction | Record 0237 |
| 3 | `A2M_HOME` moves the installation layer into a folder the person's own account writes, and the installation layer may loosen every protected setting. Section 226 says an agent that sets an environment variable cannot loosen one; this variable is the way round it. On Windows the default access list of the shared program-data folder lets any user make the folder the engine reads its installation file from (a known default; not run on a machine this round) | `a2m-actions/src/paths.rs` `Paths::under`, `settings/load.rs` `Sources::read` | High | Gap | Record 0237 |
| 4 | `policy.two_person_irreversible` is not protected. An installation that turns it on can be turned off again by the user's file, an `A2M_` variable, or a flag, unless the installation locks it | `settings/schema.rs` | High | Contradiction with section 226's rule that layers 5 to 8 only tighten | Record 0236, also decided |
| 5 | On one machine nobody can write a policy set: `policy.publish`, `policy.test`, `grant.add`, and `toolset.create` are not built, and the built-in local set holds no pre-approval rule. Every approval choice the policy engine can evaluate (by tool, module, target, environment, toolset, records, effect, hazard, whether the caller is an agent) is out of a person's reach, and `policy.pre_approval = true` lets through nothing | `a2m-policy/src/set.rs` `BuiltIn::local`, `handlers/mod.rs` | High | Gap | Ranked 2 below |
| 6 | Nothing is ever deleted. `evidence.trace_retention_days` and `evidence.audit_retention_days` are read by nothing, there is no sweep, and a comment in `control.rs` says a capture is kept "as long as traces are" | `a2m-actions/src/control.rs` `keep_capture` | High for privacy and disk | Contradiction | Record 0236; ranked 4 |
| 7 | `config.set` is in the catalog and answers `action_not_built` after the presence prompt; the loader's refusal of a loosening tells the person to use it | `settings/load.rs` `apply`, `tests/audit.rs` | Medium | Contradiction in a fix message | Record 0236 consequences; ranked 3 |
| 8 | The `ci` preset sets ten values; three of them (`logging.format`, `logging.to`, `authoring.capture.enabled`) are read by nothing, and `engine.person = "absent"` is read only for the wait for a sign-in. A job in continuous integration gets text logs in a file, not JSON on standard output | `schema.rs` `PRESETS`, `control.rs` | Medium | Contradiction | Record 0236 |
| 9 | Section 226 describes a web pack group (browser, headless, profile directory, page budget, viewport, locale, time zone, recycling) none of whose keys exists, and leaves out eleven pack keys that do | Section 226, pack manifests | Medium | Contradiction | Corrections below |
| 10 | Nothing takes effect at once. The schema marks most keys "at once" and section 226 has a table of what takes effect at once; settings are loaded once, when the engine starts, and never again | `anything2mcp/src/engine.rs` `load_settings` | Medium | Contradiction | Corrections below |

## Every declared key and its true state

The states:

- **Works**: code reads it, and a test changes it and sees the behaviour change.
- **Read, unproven**: code reads it; no test changes it. **Read in part**: code honours some of its values or part of what it says.
- **Inert**: nothing reads it, and the part it governs runs today. A value is accepted and has no effect. This is the breach of the promise.
- **Not built**: nothing reads it, and the part it governs does not exist yet. A value is accepted and nothing could act on it.

The audit an hour earlier counted about 46 unread keys by searching for each key's text. This round searched the same way, then looked at every hit and at reads by prefix (`providers.mode.`, `packs.`, `ledger.intent_window.`, `scheduler.`), through constants (`PRE_APPROVAL`, `ALLOWED_ROOTS`, `TRUST_ROOTS`), and through helpers (`count`, `minutes`, `capture`). The difference is that several hits were tests, comments, and messages, not reads. The count below is of keys with no reading code: **17 work, 23 are read but unproven or read in part, 20 are inert, 35 are not built: 95.**

| Key | Default | Read by | Test that changes it | State |
| --- | --- | --- | --- | --- |
| `profile.name` | `solo` | `settings/load.rs` `Loader::run` (layer 2); `kernel.rs`, which picks the local or server policy set | `tests/settings.rs`, the team profile sets `authoring.scout.run` | Works |
| `profile.roles` | both | nothing; one process takes both | none | Not built |
| `profile.preset` | `standard` | `load.rs` `Loader::run`, `apply_preset` | `tests/settings.rs` `a_preset_is_layer_three_...` | Works |
| `store.database` | empty | nothing; the store is always `Paths::store` | none | Inert. Section 226's continuous-integration example sets it |
| `store.blobs` | empty | nothing; blobs are always under the store | none | Inert |
| `store.free_space_floor` | 1 GiB | nothing; no free-space check exists | none | Not built |
| `vault.backend` | the build's | `anything2mcp/src/engine.rs`, `cli/start.rs` (`PassphraseRoute::of_backend`) | `tests/settings.rs` `the_vault_back_end_...`; `tests/trust_roots.rs` | Works |
| `vault.root_key` | empty | nothing | none | Not built |
| `files.allowed_roots` | `<project>` | `project.rs` `check_plain`; `anything2mcp/src/trust.rs` | `project.rs` `the_allowed_roots_bound_...`; `tests/trust_roots.rs` | Works |
| `files.max_upload_bytes` | 100 MiB | `handlers/tools.rs`, the size ceiling of a source | none | Read, unproven |
| `policy.approve_writes` | `person_each_run` | `bounds.rs` `carried` | `bounds.rs` `an_approver_the_policy_engine_has_not_...` | Works. On one machine only `person_each_run` is in force: `named_approver` stops the kernel by name, `caller_confirms` is unknown to the policy engine |
| `policy.two_person_irreversible` | off | `bounds.rs` `carried` | `bounds.rs` `two_person_irreversible_is_kept_...` | Works; not protected (finding 4) |
| `policy.max_records_per_run` | 50 | `bounds.rs` `carried` | `bounds.rs`, `tests/audit.rs` | Works |
| `policy.approval_window_minutes` | 15 | `bounds.rs`; `acts.rs` `act_window_ms`; `anything2mcp/src/local.rs` | `bounds.rs` `approval_window_minutes_is_kept_...` | Works |
| `policy.preview_freshness_seconds` | 60 | `runner.rs`, `EngineSettings::freshness_window` | none | Read, unproven |
| `policy.agent_effect_ceiling` | `irreversible` | `bounds.rs` `carried` | `bounds.rs` `agent_effect_ceiling_is_kept_...` | Works |
| `policy.pre_approval` | on | `bounds.rs` `carried`, and the guard | `bounds.rs` `pre_approval_is_kept_...`; `tests/audit.rs` | Works; on one machine there is no rule for it to allow (finding 5) |
| `policy.writes_outside_tests` | on | `bounds.rs` `carried` | `bounds.rs` `writes_outside_tests_is_kept_...` | Works |
| `identity.default_mode` | `per_user_session` | nothing | none | Not built (server) |
| `identity.allowed_modes` | `per_user_session` | nothing | none | Not built (server) |
| `sessions.keep_alive_hours` | 8 | `kernel.rs`, the keeper's settings | none that observes the keeper | Read, unproven |
| `sessions.probe_minutes` | 15 | `kernel.rs`, the keeper and `start_tending` | none | Read, unproven |
| `sessions.login_wait_minutes` | 5 | `control.rs`; `handlers/sessions.rs` | none that observes the wait | Read, unproven |
| `sessions.attempt_limit` | 1 | nothing; the keeper uses the auth file's `attempt_limit` | none | Inert: the installation's ceiling over a target's tries of a stored credential is not applied |
| `engine.idle_stop_minutes` | 30 | `anything2mcp/src/local.rs` | `a2m-local` tests `IdlePolicy`, not the setting | Read, unproven |
| `engine.in_process` | off | `cli/start.rs` `plan_for` | `tests/start_on_demand.rs` | Works |
| `engine.person` | `present` | `control.rs`, the wait for a sign-in alone | none | Read in part: section 226 says anything that would wait for a person fails at once |
| `engine.retries.transient` | 2 | `runner.rs`, `EngineSettings::transient_retries` | none through the setting | Read, unproven |
| `engine.time_limit_ceiling_seconds` | 600 | nothing; the compiler checks no installation ceiling | none | Inert |
| `engine.shadow.matches_to_promote.read` | 5 | `handlers/promote.rs` | none | Read, unproven |
| `engine.shadow.matches_to_promote.write` | 20 | `handlers/promote.rs` | none | Read, unproven |
| `engine.shadow.time_limit_ms` | 2000 | `runner.rs`, `EngineSettings::shadow_limit` | none | Read, unproven |
| `engine.promoted_writes` | `after_promotion` | nothing, though `binding.promote` is built | none | Inert: `never` is accepted and ignored |
| `ledger.intent_window.*` | the manifest's | nothing; the manifest's value is used | none | Inert |
| `modes.disabled` | none | nothing | none | Inert: a mode a person switches off keeps running |
| `scheduler.*.session_reads` | the manifest's | nothing | none | Inert |
| `scheduler.*.session_writes` | the manifest's | nothing | none | Inert |
| `scheduler.queue_length_per_target` | 100 | `kernel.rs` `sched_settings` | none | Read, unproven |
| `scheduler.priority` | `interactive_first` | nothing | none | Inert |
| `runners.lease_seconds` | 30 | `kernel.rs` `sched_settings` | none | Read in part: in one process the lease is at least 600 seconds whatever the value (`IN_PROCESS_LEASE_FLOOR`) |
| `runners.heartbeat_seconds` | 10 | `kernel.rs` `sched_settings` | none | Read, unproven |
| `runners.quarantine_seconds` | 60 | `kernel.rs` `sched_settings` | none | Read, unproven |
| `runners.certificate_hours` | 24 | nothing | none | Not built (remote runners) |
| `authoring.scout.run` | `on_add` | `handlers/scout.rs` `scout_run`, which refuses only `off` | none | Read in part: nothing tells `on_add` from `on_request` |
| `authoring.scout.try_reads` | on | `handlers/scout.rs` | none | Read, unproven |
| `authoring.scout.budget.seconds` | 300 | nothing; stage 0 of a scout says `budget_in_force: false` | none | Inert, and said so in the scout's report |
| `authoring.scout.budget.requests` | 500 | nothing | none | Inert, as above |
| `authoring.scout.budget.depth` | 3 | nothing | none | Inert, as above |
| `authoring.scout.providers` | none | nothing | none | Not built (no optional discoverer exists) |
| `authoring.capture.enabled` | on | nothing; `capture.start` is not built | none | Not built. The `ci` preset sets it |
| `authoring.capture.values` | off | nothing | none | Not built |
| `authoring.capture.limits.seconds` | 600 | nothing | none | Not built |
| `authoring.capture.limits.bytes` | 50 MiB | nothing | none | Not built |
| `authoring.capture.limits.exchanges` | 2000 | nothing | none | Not built |
| `evidence.capture` | `minimal` | `runner.rs`, the capture of a read | none through the setting | Read, unproven |
| `evidence.capture_on_writes` | `standard` | `runner.rs`, the capture of a write | `tests/pass_two.rs` `a_capture_before_the_committing_step_...` | Works |
| `evidence.trace_retention_days` | 30 | nothing; no sweep exists | none | Inert: traces and captures are kept for ever |
| `evidence.audit_retention_days` | 365 | nothing | none | Inert: the audit is kept for ever, the safe side |
| `evidence.redaction.extra` | none | nothing; the redactor masks registered secrets only | none | Inert: a person's own patterns are not masked |
| `telemetry.export` | empty | nothing | none | Not built; a value from the installation file is accepted and sends nothing |
| `telemetry.crash_reports` | off | nothing | none | Not built |
| `server.listen` | empty | nothing (tests only) | none | Not built ([decision 0169](../decisions/0169-the-first-release-is-for-one-machine.md)) |
| `server.tls.certificate` | empty | nothing | none | Not built |
| `server.tls.key` | empty | nothing | none | Not built |
| `server.base_url` | empty | nothing | none | Not built |
| `server.trusted_proxies` | none | nothing | none | Not built |
| `server.front_doors` | none | nothing | none | Not built |
| `server.local_http` | off | nothing | none | Not built |
| `server.webhooks` | none | nothing | none | Not built |
| `mcp.revisions` | both | `a2m-mcp/src/settings.rs` `McpSettings::from_settings` | door tests build `McpSettings` directly | Read, unproven |
| `auth.callers.issuer` | empty | nothing | none | Not built |
| `auth.callers.audience` | empty | nothing | none | Not built |
| `auth.callers.api_keys` | off | nothing | none | Not built |
| `tools.listed_limit` | 19 | `a2m-mcp/src/settings.rs` | none | Read, unproven |
| `tools.full_listing` | off | `a2m-mcp/src/settings.rs` | none | Read, unproven |
| `tools.results` | `concise` | nothing; there is no detailed form of a result | none | Not built |
| `tools.result_cap_bytes` | 256 KiB | `dispatch.rs` | `a2m-mcp/src/tests/checklist.rs` | Works |
| `network.egress` | `declared_surfaces` | nothing; it has one value, and the destination check is the plan's | none | Inert, and cannot be set to anything else |
| `network.allowed_hosts` | none | nothing | none | Not built |
| `network.proxy` | empty | `load.rs` `finish`, which refuses any value by name | `tests/settings.rs` `a_proxy_setting_is_refused_...` | Works, as a refusal |
| `network.trust_roots` | none | `anything2mcp/src/trust.rs` | `tests/trust_roots.rs`; `a2m-pack-http/tests/tls.rs` | Works |
| `network.private_addresses` | `refused` | nothing | none | Not built (server) |
| `providers.disabled` | none | `load.rs` `provider_settings`, then the registry | `anything2mcp/tests/command.rs` | Works |
| `providers.enabled` | none | nothing; no optional provider exists | none | Not built |
| `providers.mode.*` | none | `load.rs` `provider_settings`, then `registry.rs` | registry tests build `ProviderSettings` directly | Read, unproven |
| `providers.attach` | none | nothing | none | Not built |
| `plugins.allow_unsigned` | none | nothing | none | Not built |
| `updates.check` | off | nothing | none | Not built |
| `updates.channel` | `stable` | nothing | none | Not built |
| `alerts.destinations` | none | nothing | none | Not built |
| `logging.level` | `info` | `kernel.rs`, the telemetry's level | the loader's tests only | Read, unproven |
| `logging.format` | `text` | nothing | none | Inert. The `ci` preset sets it |
| `logging.to` | `file` | nothing; logs always go to the logs folder | none | Inert. The `ci` preset sets it |
| `locale.messages` | `en` | nothing; every message is English | none | Not built |
| `locale.display_timezone` | the machine's | nothing | none | Inert: times shown to people are not converted |

**The ten unread keys that matter most**, in order: `evidence.trace_retention_days` (everything is kept for ever, which a person set to protect their privacy cannot change); `modes.disabled` and `engine.promoted_writes` (a person who switches off a way of reaching a system, or refuses promoted bindings for writes, is ignored on the path of a run); `evidence.redaction.extra` (a person's own patterns are not masked in traces and logs); `store.database` (the documented continuous-integration example writes its store somewhere else than it says); `logging.format` and `logging.to` (the `ci` preset does not do what it says); `engine.time_limit_ceiling_seconds` (the ceiling over a target's step limits is checked nowhere); `sessions.attempt_limit` (the installation's ceiling over tries of a stored credential, which protects an account from lock-out, is not applied); and the three concurrency keys, `scheduler.*.session_reads`, `scheduler.*.session_writes`, and `ledger.intent_window.*`, which a person cannot use to go slower than a pack's manifest.

**The packs' own keys.** Ten packs declare 28 keys. Every one is read by its pack, through `PackContext::settings`; tests name `packs.command.admin_groups` (`a2m-pack-command` `the_named_groups_are_numbers_...` and `safety.rs`), `packs.http.max_response_bytes`, `packs.mcp.list_max_age_ms`, `packs.device.min_gap_ms` and `max_frames_per_run`, and `packs.stream.wait_ceiling_seconds`; this round did not check whether the others' tests change them. Every pack key takes effect at the next start. Two declare a safer direction and are not protected (`packs.command.max_timeout_ms`, `packs.command.max_output_bytes`), so a user's file may raise them up to their bounds; only `packs.command.admin_groups` is protected.

| Pack | Keys |
| --- | --- |
| `web` | `max_read_chars` 1048576; `max_snapshot_elements` 400; `browser_digests`, installation only |
| `web_probe` | `max_pages` 20; `browser_digests`, installation only |
| `http` | `max_response_bytes` 8000000 |
| `mcp` | `max_message_bytes` 4194304; `max_tools` 500; `list_max_age_ms` 10000 |
| `desktop` | `max_nodes` 2000; `max_depth` 32; `max_read_chars` 65536 |
| `command` | `max_timeout_ms` 600000; `max_output_bytes` 16777216; `admin_groups`, protected |
| `data` | `max_rows` 1000; `result_bytes` 4000000 |
| `file` | `max_bytes` 16777216; `max_rows` 200000; `max_cells` 2000000; `max_cell_bytes` 65536 |
| `stream` | `wait_ceiling_seconds` 60; `sample_count` 5; `message_ceiling_bytes` 8388608; `idle_links` 2 |
| `device` | `reply_within_ms` 1000; `min_gap_ms` 250; `max_frames_per_run` 100 |

## The dimensions

Each gap is given a kind as SCOPE.md uses them (Rule, Choice, Not yet) and a size: S is a slice, M two or three, L a phase's worth.

### 1. What something can be said for: the scope

**What exists.** Three means, each with its own scopes. Settings: the installation, the user, and the project (in practice the project of the engine's start, finding 2). Target files: per surface (`rate`, `profile`), per operation (effect, hazard, timeouts through the plan, record limits, permissions), per environment (`kind`, addresses, `test_records`, and `approve_writes`, which can only tighten), and per target (`defaults.timeouts`, `defaults.limits`, `defaults.capture`, the run's `environment.locale` and `environment.timezone`). Policy: grants and pre-approval rules over a target, a module, or a name pattern, whose conditions may name `effect`, `hazard`, `records`, `environment`, `toolset`, `target`, `module`, `tool`, and `caller_is_agent` (`CONDITION_NAMES`), and kill switches over everything, a workspace, a target, or one operation.

**What is missing.**

- **The project scope that section 226 promises** (Not yet, M). A project's file applies only when that project started the engine (finding 2). Record 0237 makes it apply to that project's requests.
- **Any policy scope at all on one machine** (Not yet, L). The policy engine evaluates every condition above; nothing on one machine writes a set (finding 5).
- **One operation, one environment, by a setting** (Choice). Settings do not name surfaces or operations, by rule 8; a person who wants "stricter in production" has three means (an environment's `approve_writes`, a policy condition on `environment`, and `policy.writes_outside_tests`) and can reach only the first and the last. Record 0237 states which means holds which.
- **One calling client** (Rule, decision [0058](../decisions/0058-a-toolkit-for-developers-with-no-catalog-and-no-preferred-agent.md) and the convention that no client is special; question 0238). The policy context has no client or connection, and the make a client claims is not trustworthy. Toolsets would give one connection its own list on a server; on one machine there is one list.
- **One network** (Not yet, designed in the [network awareness plan](network-awareness.md), M to L). Reach per recognized network, and a policy input naming the network, are designed; nothing is built.
- **One time of day** (Not yet, S once policy authoring exists). No condition name gives the time; the engine has a clock, and a condition over it is deterministic, so rule 1 does not stand in the way.
- **Told in one place** (Not yet). Section 226's scope table lists workspace, toolset, and caller scopes that no one on one machine can set, without saying so. Record 0237 is the one statement.

### 2. Approval and consent

**What exists.** Every write waits for a person on one machine (`person_each_run`), bound to the exact diff; one approval covers a whole pipeline; the window is 1 to 1,440 minutes and a window that ends is a decline; two-person approval of irreversible operations is a setting; the presence prompt of the operating system; `writes_outside_tests` and `agent_effect_ceiling` as switches.

**What is missing.**

- **Pre-approval rules a person can write** (Not yet, L): finding 5.
- **Standing approvals** (Not yet, M). Section 212 designs them for a named pipeline, caller, and time window; no type exists in `a2m-policy`. For an action that leaves nothing to read back, the question is the owner's ([0215](../decisions/0215-whether-a-person-may-pre-approve-an-action-that-leaves-nothing-to-read-back.md)), and this review decides nothing about it.
- **A condition on a value, not only a count** (Not yet, M): "updates whose amount is under a limit". Conditions see `records`, never a field. A condition over a field would compare a typed, normalized value with decimal arithmetic, never a float.
- **Being told that a run waits** (Not yet, M). On one machine the only way is the approval page; `a2m-local`'s surface says no notification is built. A person away from the page learns nothing.
- **Approving several waiting runs at once** (Not yet, S to M): not designed. Each run has its own diff; a page that lists them and takes one presence check for the ones the person ticks keeps each approval bound to its diff.
- **What the approval shows** (Choice): fixed by section 212, and right to be: an approval that a person can configure to show less is weaker.

### 3. How the engine behaves

**What exists.** Retries of `transient` failures (0 to 5); the size of a result (`tools.result_cap_bytes`); each pack's ceilings (bytes, rows, waits, frames); the shadow time limit; the capture level of reads and writes; the level of logging; queue length and lease timings; session keep-alive and probe.

**What is missing.**

- **A ceiling over a target's rate** (Not yet, S). Section 226's principle 7 says a setting can lower a target's rate budget; no key does.
- **A ceiling over a target's step time limits** (Not yet, S): `engine.time_limit_ceiling_seconds` is inert.
- **Going slower than a pack's manifest** (Not yet, S): the three concurrency keys are inert.
- **Retention** (Not yet, S to M): finding 6. Nothing is deleted: not traces, not captures, not the audit, not finished runs. A ceiling on the store's size does not exist either.
- **A person's own redaction** (Not yet, S to M). Masking is of registered secrets only; patterns and field names of a person's own are declared and unread. A pattern is matched as a literal or a field name, never as a regular expression that could run long.
- **Logging** (Not yet, S): the form and the place are inert; the size at which a log file turns over and how many are kept are fixed in `TelemetrySettings` (10 MiB, five).
- **An export of the audit for an organization's own system** (Not yet, S for a file, M for anything sent). `audit.query` is built; nothing writes the audit as lines to a file another program can collect. Sending it anywhere is a protected setting by principle 6.
- **Paging of a result** (Not yet, M): a result over the cap stops with the cap named; there is no next page.

### 4. How each pack reaches its kind of system

**What exists.** Trust roots of the person's own (`network.trust_roots`); which browser by digest (`browser_digests`); each pack's ceilings; a command's environment and working folder, which are the target's (`env.set`, `env.pass`); the time zone and locale of a run, which are the target's.

**What is missing.**

- **A proxy the person names** (Not yet, M), owed by [decision 0208](../decisions/0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md) and refused by name today. The network awareness plan reframes it as a place reached through a gateway.
- **Name resolution of the person's own** (Not yet, M): no resolver, no fixed name-to-address entries. The plan's place model is where it belongs.
- **What the product says of itself** (Rule 5 bounds it; question in "Questions for the owner"). The HTTP pack sends `anything2mcp/<version>` and nothing a person can add, such as a contact address an organization's gateway asks for. Adding is consistent with operating openly; replacing is the owner's to rule on.
- **The browser's window, size, and whether it shows** (Not yet, S). Section 226's web group does not exist; whether a window shows is decided per launch, not by the person.
- **Connections kept to a database, time to find a desktop window** (Not yet, S each): section 226 lists them as what the packs would configure; none is declared.

### 5. What an agent sees

**What exists.** How many tools are listed up front and whether all are; the result cap; the two built-in toolsets, `use` and `build`; descriptions from the target's files.

**What is missing.**

- **Toolsets on one machine** (Not yet, M): `toolset.create` and `toolset.update` are not built, so a person cannot give one task a short list of its own.
- **Client profiles as data** (Not yet, M). Section 221 and decision 0058 say a client's quirk is data in a profile; there is no format and no file. This is the answer the design gives to clients that take only so many tools.
- **The detail of a result** (Not yet, S): `tools.results` has no detailed form behind it.
- **The language** (Not yet, M): every message is English; descriptions are in whatever language the target's author wrote.
- **Names** (Choice, the naming convention in AGENTS.md): a tool's name is `{target}_{module}_{object}_{verb}`, and an alias per client would put a second name in the world for one tool. A profile may shorten the list, never rename.

### 6. Extending the product without changing it

**What the provider contract allows** (`a2m-provider` `Registrar` and `CompilerRegistrations`): drivers, discoverers, sign-in providers, source readers, provider actions, surface kinds, controls, file kinds, lowerings, lint rules, and drafting rules.

| Extension | State |
| --- | --- |
| A pack of one's own, compiled in | Built: the contract is the same for every first-party pack |
| A plug-in pack in another process | Designed ([section 202](../spec/2-kernel/202-kernel-and-packs.md)); `plugin.install` not built; roadmap F3 held. Not yet, L |
| A field type of one's own | Built, as data: a target extends a built-in type in `types.yaml` |
| A normalizer of one's own, as code | Absent: the contract has no field-type registration. Not yet; any such code on the path of a run meets rule 6 (question 0239) |
| A sign-in provider, a source reader | Built in the contract; reached only through a compiled-in pack until plug-ins exist |
| An organization's lint rules | Built in the contract (`lint_rule`); only through a pack |
| An organization's publish gates | Absent: a gate is not a registration. Not yet, M |
| Hooks before and after a run | Absent and undesigned. Code of a person's own on the path of a run meets rules 1 and 6: question 0239 |
| Templates for a new project | Absent: `init` makes one fixed project. Not yet, S |

### 7. Where things live

**What exists.** `A2M_HOME` puts every folder under one directory, which also gives several separate setups on one machine, each with its own engine lock, socket, vault entry (named by a digest of its data folder), and store. Each system's own folder conventions otherwise.

**What is missing.**

- **The store's and the blobs' place** (Not yet, S): `store.database` and `store.blobs` are inert.
- **Moving the runtime folder alone** (Not yet, S): only `A2M_HOME` moves it, with everything else.
- **Carrying a setup to another machine** (Not yet, M). Settings and targets are plain files and can be copied; the vault and the store cannot be exported, and `version.bundle` and `version.import` are not built.
- **Who may write the installation layer** (finding 3): `A2M_HOME` moves it into the person's own folder. Record 0237.

### 8. Working with configuration itself

| A person can | State |
| --- | --- |
| See every setting, its value, its layer and file and line, what each lower layer set, locks, presets, refusals, the digest | Built: `config.show` |
| Read one key's documentation | Built: `config.show <key>` |
| Change one key | Not built: `config.set` answers `action_not_built` after the presence prompt (finding 7) |
| Ask why something was refused | Built for policy: `policy.explain`; a setting the policy engine cannot work by stops the kernel naming it (`setting_not_in_force`) |
| Check a file before using it | In part: `doctor.run` loads the files in place. No command checks a file that is not yet in place |
| See what changed between two starts | In part: the audit gets `settings.changed` with the new digest, not which keys or layers |
| Get a schema an editor can use | Absent. Not yet, S |
| Read a generated page of every key | Absent: `docs.get settings` is one paragraph; section 226's tables are written by hand and have drifted (corrections below). Not yet, S |
| Be told of an unknown or misspelt key | Built: refused with file, line, and the nearest keys |
| Be told of a key for something not built | Only `network.proxy`. Record 0236 |
| Be told that the running engine holds other settings | Built for variables (`settings_differ`); not for a project file (finding 2) |

### 9. Presets and profiles

**What they really change.** `cautious` sets eight values, all read: the agent's effect ceiling to `read`, pre-approval off, ten records a run, the scout on request and not trying reads, standard capture for reads, a keep-alive of one hour. `ci` sets ten: in-process, person absent, writes only to test records, the scout off, capture recording off, JSON logs to standard output, no wait for a sign-in. Of those, the recording switch and both logging keys are read by nothing, and "person absent" stops only the wait for a sign-in. The profile changes one value: off `solo` and `workstation`, the scout runs on request. A site may add its own preset as a file; one preset at a time.

**What is missing** (each Not yet, S, as a preset of keys that work):

- **Unattended**: a machine with no person at it that serves reads to an agent and refuses every write at once rather than leaving it waiting. Today `ci` comes nearest, and it runs in-process, which an unattended engine serving a client should not.
- **Read-only**: an installation that never writes. Today it is `agent_effect_ceiling = "read"` with `pre_approval = false`; a person also wants a person's own calls held to reads, which no key gives.
- **Demonstration**: against a test stand only, short retention, nothing kept. Needs retention (ranked 4).
- **First run**: the defaults, with the scout trying reads and the person told at each stop what to do; this is the defaults, and need not be a preset.

### 10. Dimensions not on the lead's list

1. **When a change takes effect, and which engine holds it.** Every change waits for the next start; there is no reload; the schema's "at once" is not true (finding 10). And one engine serves every project of the person, with the layers 6 to 8 of whichever command started it (finding 2). A person cannot ask "what will this command run under" before running it, except by `config.show` from the same folder with the same variables. Not yet, M.
2. **Who holds the pen for each layer.** On one machine every layer but the installation's is the person's own to write, and so an agent's with a shell. Only the installation layer and the presence prompt stand between an agent and a loosening, and `A2M_HOME` and a folder any user can create remove the first (finding 3). Record 0237.
3. **The machine's own resources.** Disk: no free-space check, no ceiling on the store, nothing deleted. Memory: no browser page budget (section 226 lists one that does not exist). Logs: fixed size and count. A person on a small machine cannot bound what the product takes. Not yet, S to M.
4. **What the person is shown, in their terms.** The approval page, notices, and times: one language, the machine's time zone without conversion, no larger text or plainer form. `locale.*` is declared and inert. Not yet, M.

## The fixed frame: what is not configurable, as the code holds it

**Guardrails with no setting at all.** These have no key; a file that names one fails as an unknown key.

| What | Held in the code by | Kind |
| --- | --- | --- |
| No model or improvising step in a run | There is no such key; the engine runs a compiled plan | Rule 1 |
| An intent recorded before a write, a read-back after, reconcile before writing again | The engine loop and the ledger; no key touches either | Rule 3 |
| No ledger, no write; a store or policy that cannot be trusted stops the work | Fail closed, section 224 | Rule 3, rule 2 |
| A secret never reaches a caller, a log, a trace, a capture, or a fixture | The redactor has no off switch; cookies and authorization values are dropped at every capture level | Rule 4 |
| The browser uses only a data directory the engine made; no stealth, no spoofing, no bypass | `a2m-pack-web-browser` launch arguments; there is no key | Rule 5 |
| An unknown control, platform, or shape stops; no fallback between providers, no best-effort mode | The failure classes | Rule 7 |
| Retries: only `transient` has a count (0 to 5), and only for reads and idempotent steps | `engine.retries.transient` range; the failure classes | Choice, resting on rule 3 |
| Only a person approves; a named approver is not the caller; irreversible and hazardous runs need a person every run; a session is its owner's | `a2m-policy` `GUARDRAILS`, five forbids hashed into every set (`guardrails_digest`) | Choice resting on rule 2 for the first; rules 2 and 3 and section 213 for the next two; the convention on sessions in AGENTS.md for the last |
| The kill switch is checked first; an error in deciding is a denial; with no rule, the answer is no | `limiter.kill_switch`, `engine.evaluation_failed`, `default.deny` | Choice resting on rule 2 |
| No input chooses a host, a path, a program, or a file | The permission manifest and typed templates, section 502 | The convention in AGENTS.md, resting on rule 2 |
| A command is never a shell; a statement is never built from an input | The command and data packs | Rule 7 and section 213 |
| A tool's name is `{target}_{module}_{object}_{verb}` | The compiler | Choice (AGENTS.md naming) |
| The engine listens only on the person's own channel | `a2m-local`; no front door is built | Owner decisions 0169 and 0208 |

Section 226 also lists "the author of a version is never its only publisher" as a built-in forbid. No such forbid is in `GUARDRAILS`; on one machine the author and the publisher are the same person, by design. It is a server rule and is not built.

**Protected settings: a looser value only from the installation layer, or, once `config.set` exists, by a person through the presence prompt.** In the schema: `files.allowed_roots`; seven of the eight `policy.*` keys (all but `two_person_irreversible`, finding 4); both `engine.shadow.matches_to_promote.*`; `authoring.capture.values`; `evidence.capture_on_writes`; `evidence.audit_retention_days`; both `telemetry.*`; `server.local_http`; `server.webhooks`; `network.allowed_hosts`, `network.proxy`, `network.trust_roots`; `providers.enabled`; `providers.attach`; `plugins.allow_unsigned`; `updates.check`; `alerts.destinations`: 25 keys. And one pack key, `packs.command.admin_groups`.

**What can only tighten.** A preset, and layers 5 to 8, on any protected key (`Layer::only_tightens`); a policy set published later, on every policy setting (`Settings::tighter`); a target's environment `approve_writes`, which is the stricter of it and the installation's. Pack settings are ceilings by the contract's convention, not by a check.

**What only the installation sets.** 47 keys of scope I, among them the profile, the store, the vault, identity, the attempt limit, the time-limit ceiling, the shadow match counts, the scheduler's priority, the runners, audit retention, telemetry, every server and caller key, the MCP revisions, the whole network group, the provider switches, plug-ins, updates, and alerts. The installation file may also lock any key (`[locks] keys`) against every higher layer in both directions.

**The free space inside the frame,** in one line each: the scout and its budget, capture levels, retention, logging, keep-alive and waits, the transient retry count, tool listing and result size, the locale of what people see, each pack's ceilings, and, in a target's files, everything about the system: rates, timeouts, limits, environments, effect, hazard, permissions, field types.

## The ten additions that would most widen what a person can shape

Before any of them: make every declared key honest (record 0236). An addition built on a schema where a third of the keys do nothing would be read as one more of them.

| # | Addition | Why it ranks here | Kind and size |
| --- | --- | --- | --- |
| 1 | A project's file applies to that project's requests | It turns one of the three scopes the design promises from false to true, and is what a developer reaches for first | Not yet, M (record 0237) |
| 2 | Policy authoring on one machine: `policy.test` and `policy.publish` for a person, behind the presence prompt, with pre-approval rules, quotas, and kill switches by scope | It opens the whole approval dimension (by tool, module, environment, toolset, records, effect, hazard) that the policy engine already evaluates | Not yet, L |
| 3 | `config.set`, with the loosened value kept as a record bound to the presence check | It is the only way on one machine to loosen a protected setting without administrator rights, and the loader already tells people to use it | Not yet, M |
| 4 | Retention that deletes: traces, captures, finished runs, and audit entries after their days, and a ceiling on the store | Privacy and disk, and two inert keys become true | Not yet, S to M |
| 5 | A person's own redaction patterns and field names | A person's own sensitive data, which is not a secret the vault holds, stays out of traces and logs | Not yet, S to M |
| 6 | The form and place of logs, and the audit as lines in a file | An organization's own collectors pick up files; nothing is sent; the `ci` preset becomes true | Not yet, S |
| 7 | Toolsets on one machine and client profiles as data | The answer the design gives to clients that take only so many tools, and to a task that should see a short list | Not yet, M |
| 8 | A notification of a waiting run, approval of several at once, and standing approvals within the guardrails | Approval stops being something a person must sit at a page for | Not yet, M; standing approval of an action with nothing to read back is the owner's (0215) |
| 9 | A generated reference of every key, an editor schema, a command that checks a file before it is used, and the changed keys in the start audit | Configuration a person can work with without reading Rust | Not yet, S |
| 10 | An outbound proxy and name resolution of the person's own | Owed by decision 0208; without it, a machine behind an organization's gateway reaches nothing | Not yet, M |

## Questions for the owner

- **Record [0238](../decisions/0238-whether-a-person-may-configure-one-client-connection-apart-from-another.md).** May a person's settings or policy tell one client connection from another, by a name the person gives each connection, never by the make a client claims? Decision 0058 and the convention that no client is special stand in the way.
- **Record [0239](../decisions/0239-whether-a-persons-own-code-may-run-on-the-path-of-a-run.md).** May a person's own code run on the path of a run, as a hook or a normalizer, and in which language? Rules 1 and 6 stand in the way.
- **Without a record yet, for the lead to number if wanted.** May a person add to how the product identifies itself to a system (a contact address beside `anything2mcp/<version>`), and may they ever replace it with their own organization's registered name? Adding looks consistent with rule 5's "operate openly"; replacing touches its ban on imitating another application's client, and is the owner's.
- **Already open, not decided here.** [0215](../decisions/0215-whether-a-person-may-pre-approve-an-action-that-leaves-nothing-to-read-back.md), standing approval of an action that leaves nothing to read back.

## Lines for SCOPE.md

Not edited; for the lead. Add a section after "Building tools":

```markdown
## Configuration

| It does | It does not, today | Kind |
| --- | --- | --- |
| Takes settings from eight layers, refuses an unknown or misspelt key by name, and shows every value and the layer that set it (`config show`) | Act on every key it declares: 55 of the 95 kernel keys are read by nothing, and a value for one is accepted without effect, except `network.proxy`, which is refused ([decision 0236](docs/decisions/0236-a-declared-setting-whose-feature-is-not-built-is-refused-by-name.md)) | Not yet |
| Applies the project file of the folder the engine started in | Apply each project's own file to that project's requests ([decision 0237](docs/decisions/0237-one-statement-of-what-is-said-at-which-scope-by-which-means-and-by-whom.md)) | Not yet |
| Lets a preset, the user's file, the project's file, a variable, or a flag tighten a protected setting | Let any of them loosen one, or reach a guardrail | Rule 2, and section 226 |
| Evaluates policy rules by tool, module, target, environment, toolset, records, effect, hazard, and whether the caller is an agent | Let a person on one machine write a policy set: there is no pre-approval rule, quota, or grant of their own | Not yet |
| Keeps traces, captures, runs, and the audit on the machine | Delete any of them after their retention | Not yet |
| Changes a setting at the next start of the engine | Change one while it runs, or change one through `config set` | Not yet |
| | Tell one client connection from another in settings or policy | Rule: decision 0058. Question open for the owner: [0238](docs/decisions/0238-whether-a-person-may-configure-one-client-connection-apart-from-another.md) |
| | Run a person's own code on the path of a run, as a hook or a normalizer | Rules 1 and 6. Question open for the owner: [0239](docs/decisions/0239-whether-a-persons-own-code-may-run-on-the-path-of-a-run.md) |
```

And change one existing line. In "The machine and the network", the row "Refuses to send a credential over an unencrypted link to another machine | Reach the outside through a proxy. One the person names is owed by decision 0208 | Not yet" stays; add to its "does" side nothing. In "People and agents", after "Holds a write until a person at the machine approves it", add to the "does not" side: "Tell the person that a run waits anywhere but the approval page" (Not yet).

## Corrections section 226 needs

Not edited, because another agent is restructuring documents; for the lead. Each says what the section states and what the code does.

1. **Principle 2** says a misspelt setting can never be silently without effect. True of a misspelt key; untrue of 55 declared keys. After record 0236: "a key whose feature is not built is refused by name, with what happens instead".
2. **Principle 3** says `docs.get`, the editor schema, and `config.show` are generated from the schema. `docs.get settings` is one fixed paragraph, there is no editor schema, and per-key documentation is `config.show <key>`. It says a test fails if a key is read that the schema does not declare: the test (`every_key_the_crate_reads_...`) scans `a2m-actions/src` alone, for four accessor names, and misses the reads in `anything2mcp` and `a2m-mcp`.
3. **Principle 4**: `config.set` is not built. The start audit records `settings.changed` with the new digest only, not the keys that changed or their layers.
4. **Principle 7** says a setting can lower a target's rate budget or time limit. No key lowers a rate; `engine.time_limit_ceiling_seconds` is read by nothing. The pack ceilings (`packs.command.max_timeout_ms`, `packs.stream.wait_ceiling_seconds`, `packs.data.max_rows`, and the others) do bound targets.
5. **"What no setting can do", row 4**: "redaction rules can be added" is untrue until `evidence.redaction.extra` is read. **Row 8**: there is no forbid for the author being the only publisher in `a2m-policy`; on one machine author and publisher are one person by design; it is a server rule, not built.
6. **Layers table, layer 2**: `profile.name` is read only from the installation file and the flags of `serve`, not "any file layer". Layer 3: the preset is named by the flags, the variables, the user's file, or the installation's, never the project's (its scope is U).
7. **Layer 6**: the project file applied is the one of the folder the engine started in, for every request the engine then serves; a request's own project file is checked to parse and is not applied. Until record 0237 is built, the section should say so.
8. **Layers 7 and 8**: read once, when the engine starts. A later command's variables produce the `settings_differ` warning; its flags reach no running engine.
9. **Scopes table**: the workspace, toolset, and caller rows can be set by nobody on one machine (`policy.publish`, `grant.add`, `toolset.create`, `toolset.update` are not built). The installation row's example, `store.database`, is read by nothing.
10. **Protected settings**: the table lacks `packs.command.admin_groups`. `policy.two_person_irreversible` says "no (raising is tightening)": turning it off is a loosening, and nothing checks it (record 0236 protects it). "On one machine that channel is `config.set`": not built.
11. **Presets**: the `ci` row's "logs as JSON to standard output" and `authoring.capture.enabled = false` have no effect; "anything that would wait for a person fails at once" holds only for the wait for a sign-in.
12. **Groups**: `sessions.probe_minutes` takes minutes only, not `learned`; `tools.listed_limit` is 19; `policy.approve_writes` has scope U, and on one machine only `person_each_run` is in force; `runners.lease_seconds` is at least 600 in one process; `store.free_space_floor` has no check behind it; `network.egress` has one value.
13. **Per-pack configuration**: the web pack's table names eight keys none of which exists. The keys that exist are in the packs table above. Every pack key takes effect at the next start, and a pack key that declares a safer direction without `protected = true` is not protected.
14. **Changing configuration at run time**: nothing takes effect at once; settings are loaded once at start and never reloaded. The table should say "at the next start" for every setting until a reload exists.
15. **Configuring a target**: "the compiler refuses a value above the ceilings of the installation" holds for pack ceilings a pack checks at compile time, and not for the step time-limit ceiling.
16. **Worked examples**: the continuous-integration example's `store.database = "./ci-store.db"` has no effect.

[Section 107](../spec/1-product/107-the-bar.md), configurability, should add to its test: every declared key is read, or marked not built and refused by name.

## What this round did not read

- The bodies of most handlers beyond the lines that read settings; the policy evaluator's full code; the browser engine beyond its launch arguments; the per-system vault.
- Whether `doctor.run` reports what section 226 says it reports.
- Whether the tests of most pack settings change the setting (named above where seen).
- The Windows access list of the shared program-data folder on a real machine: finding 3's Windows half rests on its known default, not on a run.
- The guides in `a2m-actions/guides/`, section 601's profiles, section 603's networking, section 605's packaging, and the research notes.
- `config show` was not run, because the command starts an engine.
