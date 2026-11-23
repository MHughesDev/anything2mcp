# Decision records

One file per decision: the context, what was decided, what follows from it, what was rejected, and what would reopen it. Add a new numbered file for every decision that changes the design; never rewrite an old one. To reverse a decision, add a new record that supersedes it and mark the old one superseded.

A record's status is one of: **Accepted**, in force; **Proposed**, a change someone wants and has not been given, not to be built on until accepted or supported by a tested hypothesis; **Superseded**, replaced by a later record that names it.

Who may change what ([record 0055](0055-agents-plan-build-and-merge-without-waiting-for-the-owner.md)): a record marked as a design decision can be superseded by any agent with a new record that gives the evidence. A record marked as an owner decision is changed only by the owner; an agent that thinks one is wrong writes a Proposed record and carries on. A record the owner has not read says so in its status line and is in force all the same.

Records 0001 to 0054 were written when the product was named Web2MCP, and keep the names of their time. [Record 0056](0056-one-name-everywhere-anything2mcp.md) has the key from the earlier names to the present ones.

Some numbers have no record: 0118, 0125 to 0129, 0174, 0210, 0227 to 0229, and 0235 were set aside for work that did not need them, and were never written. A missing number is not a missing decision.

| # | Decision | Status |
| --- | --- | --- |
| 0001 | [Name: Web2MCP](0001-name-web2mcp.md) | Superseded by 0048 |
| 0002 | [Rust first, other languages where they fit](0002-rust-first-other-languages-where-they-fit.md) | Accepted (owner decision) |
| 0003 | [No AI at run time](0003-no-ai-at-run-time.md) | Accepted (design decision) |
| 0004 | [Rust browser engine on the DevTools protocol](0004-rust-browser-engine-on-the-devtools-protocol.md) | Accepted (design decision) |
| 0005 | [Seven modes behind one driver interface](0005-seven-modes-behind-one-driver-interface.md) | Accepted (design decision) |
| 0006 | [The runtime picks the fastest verified binding](0006-the-runtime-picks-the-fastest-verified-binding.md) | Accepted (design decision) |
| 0007 | [New bindings run in shadow before promotion](0007-new-bindings-run-in-shadow-before-promotion.md) | Accepted (design decision) |
| 0008 | [Control plane plus runners](0008-control-plane-plus-runners.md) | Accepted (design decision) |
| 0009 | [Four identity modes on a shared server](0009-four-identity-modes-on-a-shared-server.md) | Accepted (design decision) |
| 0010 | [Toolsets, and few tools listed up front](0010-toolsets-and-few-tools-listed-up-front.md) | Accepted (design decision) |
| 0011 | [Stateless Streamable HTTP and an OAuth resource server](0011-stateless-streamable-http-and-an-oauth-resource-server.md) | Accepted (design decision) |
| 0012 | [Approvals happen out of reach of the agent and bind to the diff](0012-approvals-happen-out-of-reach-of-the-agent-and-bind-to-the-diff.md) | Accepted (design decision) |
| 0013 | [Writes go through the interface until a faster binding is proven](0013-writes-go-through-the-interface-until-a-faster-binding-is-proven.md) | Accepted (design decision) |
| 0014 | [Declarative files: YAML targets, TOML settings, CEL expressions](0014-declarative-files-yaml-targets-toml-settings-cel-expressions.md) | Accepted (design decision) |
| 0015 | [SQLite locally, Postgres on servers, and a built-in ledger](0015-sqlite-locally-postgres-on-servers-and-a-built-in-ledger.md) | Accepted (design decision) |
| 0016 | [Plug-ins are separate processes](0016-plug-ins-are-separate-processes.md) | Accepted (design decision) |
| 0017 | [Operate openly: no evasion of bot defenses or MFA](0017-operate-openly-no-evasion-of-bot-defenses-or-mfa.md) | Accepted (design decision) |
| 0018 | [License: Apache-2.0](0018-license-apache-2-0.md) | Superseded by 0057 |
| 0019 | [Priority order: safe, correct, fast, easy](0019-priority-order-safe-correct-fast-easy.md) | Accepted (owner decision) |
| 0020 | [Every write is verified, and resume reconciles first](0020-every-write-is-verified-and-resume-reconciles-first.md) | Accepted (design decision) |
| 0021 | [No real application is named in the repository](0021-no-real-application-is-named-in-the-repository.md) | Accepted (owner decision) |
| 0022 | [Published package names](0022-published-package-names.md) | Superseded by 0056 |
| 0023 | [Commands and files first for building, MCP tools second, one registry for both](0023-commands-and-files-first-for-building-mcp-tools-second-one-registry-for-both.md) | Accepted (design decision) |
| 0024 | [One local engine per user, reached over a socket or named pipe](0024-one-local-engine-per-user-reached-over-a-socket-or-named-pipe.md) | Accepted (design decision) |
| 0025 | [pip and npm packages ship the binary with a thin SDK; no in-process bindings yet](0025-pip-and-npm-packages-ship-the-binary-with-a-thin-sdk-no-in-process-bindings-yet.md) | Accepted (design decision) |
| 0026 | [Local approvals come from the engine, with an operating-system user-presence check](0026-local-approvals-come-from-the-engine-with-an-operating-system-user-presence-check.md) | Accepted (design decision) |
| 0027 | [Ledger: one row per commit point, with append-only events beside current state](0027-ledger-one-row-per-commit-point-with-append-only-events-beside-current-state.md) | Accepted (design decision) |
| 0028 | [Runner link: mutually authenticated HTTPS with long polls, and the same interface in memory](0028-runner-link-mutually-authenticated-https-with-long-polls-and-the-same-interface-in-memory.md) | Accepted (design decision) |
| 0029 | [Lost leases: stop early, quarantine, reconcile, verify again](0029-lost-leases-stop-early-quarantine-reconcile-verify-again.md) | Accepted (design decision) |
| 0030 | [Where session secrets live](0030-where-session-secrets-live.md) | Accepted (design decision) |
| 0031 | [The browser always runs with a profile the engine owns](0031-the-browser-always-runs-with-a-profile-the-engine-owns.md) | Accepted (design decision) |
| 0032 | [One live session per identity, shared by tabs, with a single renewer](0032-one-live-session-per-identity-shared-by-tabs-with-a-single-renewer.md) | Accepted (design decision) |
| 0033 | [Policy decisions use Cedar, with grants compiled to it](0033-policy-decisions-use-cedar-with-grants-compiled-to-it.md) | Superseded by 0082 |
| 0034 | [Policy is a versioned, tested artifact, and every decision is recorded](0034-policy-is-a-versioned-tested-artifact-and-every-decision-is-recorded.md) | Accepted (design decision) |
| 0035 | [A user's session is used only by that user and agents acting for them](0035-a-user-s-session-is-used-only-by-that-user-and-agents-acting-for-them.md) | Accepted (owner decision) |
| 0036 | [Target files are compiled into a hashed plan before anything runs](0036-target-files-are-compiled-into-a-hashed-plan-before-anything-runs.md) | Accepted (design decision) |
| 0037 | [Evidence: events, a step trace, and artifacts, redacted at capture](0037-evidence-events-a-step-trace-and-artifacts-redacted-at-capture.md) | Accepted (design decision) |
| 0038 | [Vault keys in four layers, with the root in the OS store or a key service](0038-vault-keys-in-four-layers-with-the-root-in-the-os-store-or-a-key-service.md) | Accepted (design decision) |
| 0039 | [Local approval: a diff page for the moment, then an operating-system prompt](0039-local-approval-a-diff-page-for-the-moment-then-an-operating-system-prompt.md) | Accepted (design decision) |
| 0040 | [Nothing leaves the machine unless switched on](0040-nothing-leaves-the-machine-unless-switched-on.md) | Accepted (design decision) |
| 0041 | [Identity strategies make creates and dependent records resumable](0041-identity-strategies-make-creates-and-dependent-records-resumable.md) | Accepted (design decision) |
| 0042 | [Design for an agent that has been steered](0042-design-for-an-agent-that-has-been-steered.md) | Accepted (design decision) |
| 0043 | [The engine fails closed, with three health states](0043-the-engine-fails-closed-with-three-health-states.md) | Accepted (design decision) |
| 0044 | [A published version becomes current in stages](0044-a-published-version-becomes-current-in-stages.md) | Accepted (design decision) |
| 0045 | [The workspace starts with every crate, and the audit enforces the layering](0045-the-workspace-starts-with-every-crate-and-the-audit-enforces-the-layering.md) | Accepted (design decision) |
| 0046 | [The pip name is held by a placeholder, published only when a version tag is pushed](0046-the-pip-name-is-held-by-a-placeholder-published-on-a-version-tag.md) | Accepted (design decision) |
| 0047 | [The test application is a small server in the workspace, and the licence list is closed](0047-the-test-application-is-a-small-server-in-the-workspace-with-a-closed-licence-list.md) | Accepted (design decision) |
| 0048 | [The product becomes Anything2MCP, a universal runtime built on the same core](0048-the-product-becomes-anything2mcp.md) | Accepted (owner decision) |
| 0049 | [A neutral kernel, and packs behind one provider contract](0049-a-neutral-kernel-and-packs-behind-one-provider-contract.md) | Accepted (design decision) |
| 0050 | [Seven interaction archetypes, and modes that arrive with packs](0050-seven-archetypes-and-modes-that-arrive-with-packs.md) | Accepted (design decision) |
| 0051 | [Findings, and an evidence ladder from inferred to approved](0051-findings-and-the-evidence-ladder.md) | Accepted (design decision) |
| 0052 | [Effects are declared and enforced, and a hazard raises the approval](0052-effects-are-enforced-and-hazards-raise-the-approval.md) | Accepted (design decision) |
| 0053 | [REA is studied for its method, and used only as an optional provider](0053-rea-is-studied-for-method-and-used-only-as-an-optional-provider.md) | Accepted (design decision) |
| 0054 | [Devices are reached through their own interfaces, and firmware only through the device's own update](0054-devices-through-their-own-interfaces-and-firmware-through-the-devices-own-update.md) | Accepted (design decision) |
| 0055 | [Agents plan, build, and merge without waiting for the owner](0055-agents-plan-build-and-merge-without-waiting-for-the-owner.md) | Accepted (owner decision) |
| 0056 | [One name everywhere: `anything2mcp`, with `a2m` for the parts](0056-one-name-everywhere-anything2mcp.md) | Accepted (design decision) |
| 0057 | [The licence is MIT](0057-the-licence-is-mit.md) | Accepted (owner decision) |
| 0058 | [A toolkit for developers, with no catalog and no preferred agent](0058-a-toolkit-for-developers-with-no-catalog-and-no-preferred-agent.md) | Accepted (owner decision) |
| 0059 | [The scout drafts and proves reads by itself, and three things stay with the developer](0059-the-scout-drafts-and-proves-reads-and-three-things-stay-with-the-developer.md) | Accepted (design decision) |
| 0060 | [A mode is a registered name, and the archetype is the closed type](0060-a-mode-is-a-registered-name-and-the-archetype-is-the-closed-type.md) | Accepted (design decision) |
| 0061 | [The layers are redrawn so that every needed dependency is allowed](0061-the-layers-are-redrawn-so-that-every-needed-dependency-is-allowed.md) | Accepted (design decision) |
| 0062 | [Words that arrive with packs are registered names, and packs share data and never code](0062-words-that-arrive-with-packs-are-registered-names-and-packs-share-data-never-code.md) | Accepted (design decision) |
| 0063 | [The kernel's interfaces, as the crate designs settle them](0063-the-kernels-interfaces-as-the-crate-designs-settle-them.md) | Accepted (design decision) |
| 0064 | [Five further packs are designed before they are built](0064-five-further-packs-are-designed-before-they-are-built.md) | Accepted (design decision) |
| 0065 | [The specification is rebuilt in parts, and the first design is archived](0065-the-specification-is-rebuilt-in-parts-and-the-first-design-is-archived.md) | Accepted (design decision) |
| 0066 | [Configuration in layers that may only tighten, and a bar that is measured](0066-configuration-in-layers-that-may-only-tighten-and-a-bar-that-is-measured.md) | Accepted (design decision) |
| 0067 | [The kernel is built layer by layer, in parallel, and the first driver through the loop is the HTTP pack's](0067-the-kernel-is-built-layer-by-layer-in-parallel-and-the-first-driver-is-the-http-packs.md) | Accepted (design decision) |
| 0068 | [Values, names, and identifiers, as the core crate settles them](0068-values-names-and-identifiers-as-the-core-crate-settles-them.md) | Accepted (design decision) |
| 0069 | [The plan is canonical JSON, hashed whole, and keyed digests are per workspace](0069-the-plan-is-canonical-json-hashed-whole-and-keyed-digests-are-per-workspace.md) | Accepted (design decision) |
| 0070 | [The browser engine stands on its own client, over the debugging pipe](0070-the-browser-engine-stands-on-its-own-client-over-the-debugging-pipe.md) | Accepted (design decision) |
| 0071 | [The MCP server stands on the official SDK behind a seam, and serves two revisions](0071-the-mcp-server-stands-on-the-official-sdk-behind-a-seam-and-serves-two-revisions.md) | Accepted (design decision) |
| 0072 | [Expressions are a CEL subset, on the engine's own parser, checker, and evaluator](0072-expressions-are-a-cel-subset-on-the-engines-own-parser-and-evaluator.md) | Accepted (design decision) |
| 0073 | [Decimals use operators, division is a function, and how a field type normalizes](0073-decimals-use-operators-division-is-a-function-and-how-a-field-type-normalizes.md) | Accepted (design decision) |
| 0074 | [The vault's cipher, key stores, and other outside crates](0074-the-vaults-cipher-key-stores-and-other-outside-crates.md) | Accepted (design decision) |
| 0075 | [The vault's interface, as building it settled it](0075-the-vaults-interface-as-building-it-settled-it.md) | Accepted (design decision) |
| 0076 | [One trace record per step, and a signature chained over record digests](0076-one-trace-record-per-step-and-a-signature-chained-over-record-digests.md) | Accepted (design decision) |
| 0077 | [Logs through the `tracing` crate with our own subscriber, and spans through a handle](0077-logs-through-the-tracing-crate-with-our-own-subscriber-and-spans-through-a-handle.md) | Accepted (design decision) |
| 0078 | [Target files are read by an event parser into our own located tree, and checked by our own schema subset](0078-target-files-are-read-by-an-event-parser-into-our-own-located-tree-and-checked-by-our-own-schema-subset.md) | Accepted (design decision) |
| 0079 | [The compiler's registration points, and the file formats, as the code settles them](0079-the-compilers-registration-points-and-the-file-formats-as-the-code-settles-them.md) | Accepted (design decision) |
| 0080 | [The store is one SQLite file, and a crate reaches its own tables through the transaction](0080-the-store-is-one-sqlite-file-and-a-crate-reaches-its-own-tables-through-the-transaction.md) | Accepted (design decision) |
| 0081 | [What the ledger holds as data: the token, doubt, locks, and rule 3](0081-what-the-ledger-holds-as-data-the-token-doubt-locks-and-rule-three.md) | Accepted (design decision) |
| 0082 | [The decision core is the policy crate's own evaluator](0082-the-decision-core-is-the-policy-crates-own-evaluator.md) | Accepted (design decision) |
| 0083 | [The policy engine's interface, as building it settled it](0083-the-policy-engines-interface-as-building-it-settled-it.md) | Accepted (design decision) |
| 0084 | [The session keeper's outside crates, and TLS for identity services](0084-the-session-keepers-outside-crates-and-tls-for-identity-services.md) | Accepted (design decision) |
| 0085 | [The session keeper's interface, as building it settled it](0085-the-session-keepers-interface-as-building-it-settled-it.md) | Accepted (design decision) |
| 0086 | [The source readers are our own, over one outside crate that inflates](0086-the-source-readers-are-our-own-over-one-outside-crate-that-inflates.md) | Accepted (design decision) |
| 0087 | [The source readers' interface, as building it settled it](0087-the-source-readers-interface-as-building-it-settled-it.md) | Accepted (design decision) |
| 0088 | [The provider contract's outside crates: a TOML parser, a descriptor helper, safe process calls, and the Windows calls](0088-the-provider-contracts-outside-crates.md) | Accepted (design decision) |
| 0089 | [The provider contract as building it settled it](0089-the-provider-contract-as-building-it-settled-it.md) | Accepted (design decision) |
| 0090 | [The supervisor ends a group only while its leader is unreaped, and its links are started by three calls](0090-the-supervisor-ends-a-group-only-while-its-leader-is-unreaped.md) | Accepted (design decision) |
| 0091 | [The engine loop, as its first two passes settled it](0091-the-engine-loop-as-its-first-two-passes-settled-it.md) | Accepted (design decision) |
| 0092 | [A record that waits for an earlier record is planned under its place in the run](0092-a-record-that-waits-for-an-earlier-record-is-planned-under-its-place-in-the-run.md) | Accepted (design decision) |
| 0093 | [The scheduler's interface, as building it settled it](0093-the-schedulers-interface-as-building-it-settled-it.md) | Accepted (design decision) |
| 0094 | [The action layer's interface, as building it settled it](0094-the-action-layers-interface-as-building-it-settled-it.md) | Accepted (design decision) |
| 0095 | [The settings loader and the in-process runner, as built](0095-the-settings-loader-and-the-in-process-runner-as-built.md) | Accepted (design decision) |
| 0096 | [The HTTP pack's endpoint entry, steps, and payload, as building them settled them](0096-the-http-packs-endpoint-entry-steps-and-payload-as-building-them-settled-them.md) | Accepted (design decision) |
| 0097 | [What the provider contract could not give the HTTP driver, and what the pack does instead](0097-what-the-provider-contract-could-not-give-the-http-driver-and-what-the-pack-does-instead.md) | Accepted (design decision) |
| 0098 | [The `cli` mode is real, and a program is a surface](0098-the-cli-mode-is-real-and-a-program-is-a-surface.md) | Accepted (design decision) |
| 0099 | [The command pack as building it settled it, and what the kernel could not give it](0099-the-command-pack-as-building-it-settled-it.md) | Accepted (design decision) |
| 0100 | [The mode `sql`, and the data pack's contract as building it settled it](0100-the-mode-sql-and-the-data-packs-contract-as-building-it-settled-it.md) | Accepted (design decision) |
| 0101 | [What the data pack refuses, and how it names it](0101-what-the-data-pack-refuses-and-how-it-names-it.md) | Accepted (design decision) |
| 0102 | [The `file` mode, and what the file pack registers](0102-the-file-mode-and-what-the-file-pack-registers.md) | Accepted (design decision) |
| 0103 | [The file pack, as building it settled it](0103-the-file-pack-as-building-it-settled-it.md) | Accepted (design decision) |
| 0104 | [A wire protocol is one interface inside the stream pack, and the first standard one is still to be chosen](0104-a-wire-protocol-is-one-interface-inside-the-stream-pack-and-the-first-standard-one-is-still-to-be-chosen.md) | Accepted (design decision) |
| 0105 | [The `stream` mode, and the stream pack as building it settled it](0105-the-stream-mode-and-the-stream-pack-as-building-it-settled-it.md) | Accepted (design decision) |
| 0106 | [The device pack speaks its first protocol itself, and checks a range before a frame is built](0106-the-device-pack-speaks-its-first-protocol-itself-and-checks-a-range-before-a-frame-is-built.md) | Accepted (design decision) |
| 0107 | [The `device` mode, and the device pack as building it settled it](0107-the-device-mode-and-the-device-pack-as-building-it-settled-it.md) | Accepted (design decision) |
| 0108 | [An upstream MCP server is reached only through a pinned mapping, by a client of the pack's own](0108-an-upstream-mcp-server-is-reached-only-through-a-pinned-mapping-by-a-client-of-the-packs-own.md) | Accepted (design decision) |
| 0109 | [The MCP pack as building it settled it](0109-the-mcp-pack-as-building-it-settled-it.md) | Accepted (design decision) |
| 0110 | [The browser engine's interface as building it settled it](0110-the-browser-engines-interface-as-building-it-settled-it.md) | Accepted (design decision) |
| 0111 | [An observing tab's lock as built, and what the browser was measured to do](0111-an-observing-tabs-lock-as-built-and-what-the-browser-was-measured-to-do.md) | Accepted (design decision) |
| 0112 | [The `ui` driver as built: five step kinds, and a commit that waits for its confirmation](0112-the-ui-driver-as-built-five-step-kinds-and-a-commit-that-waits-for-its-confirmation.md) | Accepted (design decision) |
| 0113 | [A screen's fingerprint is a section, absent is a variant, and a table is read by its headings](0113-a-screens-fingerprint-is-a-section-absent-is-a-variant-and-a-table-is-read-by-its-headings.md) | Accepted (design decision) |
| 0114 | [The web probe as built: three discoverers over an observing tab](0114-the-web-probe-as-built-three-discoverers-over-an-observing-tab.md) | Accepted (design decision) |
| 0115 | [The MCP endpoint as built: checks in front of the SDK, and a redactor on the way out](0115-the-mcp-endpoint-as-built-checks-in-front-of-the-sdk-and-a-redactor-on-the-way-out.md) | Accepted (design decision) |
| 0116 | [The REST router takes its caller from the mount and chooses its status in one table](0116-the-rest-router-takes-its-caller-from-the-mount-and-chooses-its-status-in-one-table.md) | Accepted (design decision) |
| 0117 | [The local channel and the person surface on one machine, as built](0117-the-local-channel-and-the-person-surface-on-one-machine-as-built.md) | Accepted (design decision) |
| 0119 | [The product crate, as building it settled it](0119-the-product-crate-as-building-it-settled-it.md) | Accepted (design decision) |
| 0120 | [The desktop pack stands on each system's own interface, through thin bindings](0120-the-desktop-pack-stands-on-each-systems-own-interface-through-thin-bindings.md) | Accepted (design decision) |
| 0121 | [The plan carries what the run time needs from target files](0121-the-plan-carries-what-the-run-time-needs-from-target-files.md) | Accepted (design decision) |
| 0122 | [Small things the core crate gives its builders, and a secret is wiped when dropped](0122-small-things-the-core-crate-gives-its-builders-and-a-secret-is-wiped-when-dropped.md) | Accepted (design decision) |
| 0123 | [The Linux Secret Service back end is left out until its store crate stands on maintained crates](0123-the-linux-secret-service-back-end-is-left-out-until-its-store-crate-is-maintained.md) | Accepted (design decision) |
| 0124 | [The plan declares a surface's programs, and a schema checks run-time values](0124-the-plan-declares-a-surfaces-programs-and-a-schema-checks-run-time-values.md) | Accepted (design decision) |
| 0130 | [The MCP protocol layer is our own, because the SDK brings an unmaintained crate](0130-the-mcp-protocol-layer-is-our-own-because-the-sdk-brings-an-unmaintained-crate.md) | Accepted (design decision) |
| 0131 | [The front doors joined: `serve`, the thin client, and the bridge](0131-the-front-doors-joined-serve-the-thin-client-and-the-bridge.md) | Accepted (design decision) |
| 0132 | [The action registry's second pass: a decline ends the run, and a wait reads the scheduler's own table](0132-the-action-registrys-second-pass-a-decline-ends-the-run-and-a-wait-reads-the-schedulers-own-table.md) | Accepted (design decision) |
| 0133 | [The presence prompt on each operating system, and what it is said to prove](0133-the-presence-prompt-on-each-operating-system-and-what-it-is-said-to-prove.md) | Accepted (design decision) |
| 0134 | [One browser starts at a time, and a silent start is slow, not foreign](0134-one-browser-starts-at-a-time-and-a-silent-start-is-slow-not-foreign.md) | Accepted (design decision) |
| 0135 | [The wheel carries the binary and a thin client, and npm uses platform packages](0135-the-wheel-carries-the-binary-and-a-thin-client-and-npm-uses-platform-packages.md) | Accepted (design decision) |
| 0136 | [The MCP pack refuses free text, reads its list again before a write, and passes on only listed members](0136-the-mcp-pack-refuses-free-text-reads-its-list-again-before-a-write-and-passes-on-only-listed-members.md) | Accepted (design decision) |
| 0137 | [A device is held to its serial, a write frame is recorded before it is sent, and a run has a frame ceiling](0137-a-device-is-held-to-its-serial-a-write-frame-is-recorded-before-it-is-sent-and-a-run-has-a-frame-ceiling.md) | Accepted (design decision) |
| 0138 | [The vault and the session keeper after their safety audit](0138-the-vault-and-the-session-keeper-after-their-safety-audit.md) | Accepted (design decision) |
| 0139 | [A write that was sent is never settled by an answer that is not the application's verdict](0139-a-write-that-was-sent-is-never-settled-by-an-answer-that-is-not-the-applications-verdict.md) | Accepted (design decision) |
| 0140 | [The command pack after its safety audit](0140-the-command-pack-after-its-safety-audit.md) | Accepted (design decision) |
| 0141 | [The data pack holds a statement to one read or one keyed write, and a connection to one file](0141-the-data-pack-holds-a-statement-to-one-read-or-one-keyed-write-and-a-connection-to-one-file.md) | Accepted (design decision) |
| 0142 | [The presence prompt's interface and the Linux prompt, as built](0142-the-presence-prompt-interface-and-the-linux-prompt-as-built.md) | Accepted (design decision) |
| 0143 | [The browser engine after its safety audit](0143-the-browser-engine-after-its-safety-audit.md) | Accepted (design decision) |
| 0144 | [The web drivers and discoverers after their safety audit](0144-the-web-drivers-and-discoverers-after-their-safety-audit.md) | Accepted (design decision) |
| 0145 | [Only the policy engine clears a write, and the ledger reads the whole clearance](0145-only-the-policy-engine-clears-a-write-and-the-ledger-reads-the-whole-clearance.md) | Accepted (design decision) |
| 0146 | [The macOS and Windows presence prompts, as built](0146-the-macos-and-windows-presence-prompts-as-built.md) | Accepted (design decision) |
| 0147 | [The engine after its safety audit](0147-the-engine-after-its-safety-audit.md) | Accepted (design decision) |
| 0148 | [The provider contract and the scheduler after their safety audit](0148-the-provider-contract-and-the-scheduler-after-their-safety-audit.md) | Accepted (design decision) |
| 0149 | [The file pack after its safety audit](0149-the-file-pack-after-its-safety-audit.md) | Accepted (design decision) |
| 0150 | [A take records its message before it acknowledges, and an answer with no rule leaves a write in doubt](0150-a-take-records-its-message-before-it-acknowledges-and-an-answer-with-no-rule-leaves-a-write-in-doubt.md) | Accepted (design decision) |
| 0151 | [The doors take up the action registry's second pass](0151-the-doors-take-up-the-action-registrys-second-pass.md) | Accepted (design decision) |
| 0152 | [The action registry and telemetry after their safety audit](0152-the-action-registry-and-telemetry-after-their-safety-audit.md) | Accepted (design decision) |
| 0153 | [What the compiler, the plan, and the redactor hold by themselves, after the safety audit](0153-what-the-compiler-the-plan-and-the-redactor-hold-by-themselves-after-the-safety-audit.md) | Accepted (design decision) |
| 0154 | [The HTTP, command, and MCP packs state what saves, what writes, and which key is sent](0154-the-http-command-and-mcp-packs-state-what-saves-what-writes-and-which-key-is-sent.md) | Accepted (design decision) |
| 0155 | [Four packs state what saves, what writes, and when their guard lifts](0155-four-packs-state-what-saves-what-writes-and-when-their-guard-lifts.md) | Accepted (design decision) |
| 0156 | [The clients are generated from the served document, and reach Windows by the pipe](0156-the-clients-are-generated-from-the-served-document-and-reach-windows-by-the-pipe.md) | Accepted (design decision) |
| 0157 | [TLS is switched on with the provider stated in each client](0157-tls-is-switched-on-with-the-provider-stated-in-each-client.md) | Accepted (design decision) |
| 0158 | [The product crate and the local engine after their safety audit](0158-the-product-crate-and-the-local-engine-after-their-safety-audit.md) | Accepted (design decision) |
| 0159 | [A lost answer is a write in doubt at the doors and in the clients](0159-a-lost-answer-is-a-write-in-doubt-at-the-doors-and-in-the-clients.md) | Accepted (design decision) |
| 0160 | [An idempotency key gives back its run, and `run.list` shows a caller's own runs](0160-an-idempotency-key-gives-back-its-run-and-run-list-shows-a-callers-own-runs.md) | Accepted (design decision) |
| 0161 | [The plan and the engine after the packs were built](0161-the-plan-and-the-engine-after-the-packs-were-built.md) | Accepted (design decision) |
| 0162 | [The provider contract's second pass: what a step is handed, and what it hands back](0162-the-provider-contracts-second-pass-what-a-step-is-handed-and-hands-back.md) | Accepted (design decision) |
| 0163 | [A person's act an agent asks for is carried out on a page of the person surface](0163-a-persons-act-an-agent-asks-for-is-carried-out-on-a-page-of-the-person-surface.md) | Accepted (design decision) |
| 0164 | [The authoring actions served through the engine](0164-the-authoring-actions-served-through-the-engine.md) | Accepted (design decision) |
| 0165 | [A client starts the engine on demand, detached, and gives it the passphrase on a pipe](0165-a-client-starts-the-engine-on-demand-detached-and-gives-it-the-passphrase-on-a-pipe.md) | Accepted (design decision) |
| 0166 | [HTTPS in the product: trust roots at engine start, and one maker of clients](0166-https-in-the-product-trust-roots-at-engine-start-and-one-maker-of-clients.md) | Accepted (design decision) |
| 0167 | [The stream pack speaks NATS with JetStream, through a client of its own](0167-the-stream-pack-speaks-nats-with-jetstream-through-a-client-of-its-own.md) | Accepted (design decision) |
| 0168 | [The data pack reaches a server database with the guarantees of an embedded file](0168-the-data-pack-reaches-a-server-database-with-the-guarantees-of-an-embedded-file.md) | Accepted (design decision) |
| 0169 | [The first release is for one machine](0169-the-first-release-is-for-one-machine.md) | Accepted (owner decision) |
| 0170 | [The scout identifies each surface with one discovery job and stores each stage](0170-the-scout-identifies-each-surface-with-one-discovery-job-and-stores-each-stage.md) | Accepted (design decision) |
| 0171 | [The scout's report is hashed data, with the system's words kept apart](0171-the-scouts-report-is-hashed-data-with-the-systems-words-kept-apart.md) | Accepted (design decision) |
| 0172 | [The ledger ends a record on an acknowledgement, and keeps what the system said](0172-the-ledger-ends-a-record-on-an-acknowledgement-and-keeps-what-the-system-said.md) | Accepted (design decision) |
| 0173 | [The stream and file packs take up the kernel's second pass](0173-the-stream-and-file-packs-take-up-the-kernels-second-pass.md) | Accepted (design decision) |
| 0175 | [The data and device packs take up the second pass of the provider contract](0175-the-data-and-device-packs-take-up-the-second-pass-of-the-provider-contract.md) | Accepted (design decision) |
| 0178 | [An effect left as an open question is undecided, not a write](0178-an-effect-left-as-an-open-question-is-undecided-not-a-write.md) | Accepted (design decision) |
| 0176 | [The action registry takes up the kernel's second pass](0176-the-action-registry-takes-up-the-kernels-second-pass.md) | Accepted (design decision) |
| 0177 | [A target's offline tests run against a replay of their fixtures](0177-a-targets-offline-tests-run-against-a-replay-of-their-fixtures.md) | Accepted (design decision) |
| 0179 | [What each pack drafts, so that a scout ends with reads that work](0179-what-each-pack-drafts-so-that-a-scout-ends-with-reads-that-work.md) | Accepted (design decision) |
| 0183 | [The web pack drafts a read of each table a snapshot names](0183-the-web-pack-drafts-a-read-of-each-table-a-snapshot-names.md) | Accepted (design decision) |
| 0186 | [The release workflow builds, checks, and publishes the npm packages](0186-the-release-workflow-builds-checks-and-publishes-the-npm-packages.md) | Accepted (design decision) |
| 0181 | [The product and the local engine take up what the kernel now offers](0181-the-product-and-the-local-engine-take-up-what-the-kernel-now-offers.md) | Accepted (design decision) |
| 0182 | [The file, data, and stream packs after the first drafted reads](0182-the-file-data-and-stream-packs-after-the-first-drafted-reads.md) | Accepted (design decision) |
| 0187 | [The web pack matches a screen by its address, reads by key, and reads labelled values](0187-the-web-pack-matches-a-screen-by-its-address-and-reads-by-key.md) | Accepted (design decision) |
| 0180 | [A target's offline tests cover writes, and nothing is published unless they pass](0180-a-targets-offline-tests-cover-writes-and-gate-the-publish.md) | Accepted (design decision) |
| 0184 | [The data and stream packs after their second safety audit](0184-the-data-and-stream-packs-after-their-second-safety-audit.md) | Accepted (design decision) |
| 0188 | [Credentials over plain links, drafted workbooks, and published device maps](0188-credentials-over-plain-links-drafted-workbooks-and-published-device-maps.md) | Accepted (design decision) |
| 0185 | [A record with no key is never proved absent by one look](0185-a-record-with-no-key-is-never-proved-absent-by-one-look.md) | Accepted (design decision) |
| 0190 | [The `page_http` mode reads from inside the page, in an observing tab](0190-the-page-http-mode-reads-from-inside-the-page-in-an-observing-tab.md) | Accepted (design decision) |
| 0192 | [A served engine hands no root of the project that started it, and its clients start it and compare its settings](0192-a-served-engine-hands-no-root-of-the-project-that-started-it-and-clients-start-and-compare.md) | Accepted (design decision) |
| 0189 | [A developer's first hour runs without hand steps](0189-a-developers-first-hour-runs-without-hand-steps.md) | Accepted (design decision) |
| 0193 | [The core, the contract, the compiler, and the scheduler after the kernel's second pass](0193-the-core-the-contract-the-compiler-and-the-scheduler-after-the-second-pass.md) | Accepted (design decision) |
| 0194 | [The packs, the product, and the local surface take up the kernel's second pass](0194-the-packs-the-product-and-the-local-surface-take-up-the-kernels-second-pass.md) | Accepted (design decision) |
| 0197 | [No client takes a proxy from the environment, and a stand's protocol is not shipped](0197-no-client-takes-a-proxy-from-the-environment-and-stand-only-protocols.md) | Accepted (design decision) |
| 0191 | [The release profile builds one code unit, with whole-program optimisation, for size](0191-the-release-profile-builds-one-code-unit-with-whole-program-optimisation-for-size.md) | Accepted (design decision) |
| 0195 | [A read never runs where the page may write, and a page keeps only its structure](0195-a-read-never-runs-where-the-page-may-write-and-a-page-keeps-only-its-structure.md) | Accepted (design decision) |
| 0196 | [The engine and the registry after the acknowledgement ruling and the first hour](0196-the-engine-and-the-registry-after-the-acknowledgement-and-the-first-hour.md) | Accepted (design decision) |
| 0198 | [The desktop pack's first slice reads Linux applications through a look-only back end](0198-the-desktop-pack-reads-linux-applications-through-a-look-only-back-end.md) | Accepted (design decision) |
| 0199 | [The halves left for each other are joined](0199-the-halves-left-for-each-other-are-joined.md) | Accepted (design decision) |
| 0202 | [The desktop pack's control has a name of its own](0202-the-desktop-packs-control-has-a-name-of-its-own.md) | Accepted (design decision) |
| 0203 | [Two packs may each draft a file kind of one name, each for its own surfaces](0203-two-packs-may-each-draft-a-file-kind-of-one-name-for-their-own-surfaces.md) | Accepted (design decision) |
| 0204 | [One plain form of a path, a pipe that is waited for, and a socket path that fits](0204-one-plain-form-of-a-path-a-pipe-that-is-waited-for-and-a-socket-path-that-fits.md) | Accepted (design decision) |
| 0201 | [A resume is never lost, the browser takes no proxy, and no product build speaks a stand's protocol](0201-a-resume-is-never-lost-the-browser-takes-no-proxy-and-no-product-build-speaks-a-stands-protocol.md) | Accepted (design decision) |
| 0200 | [Evidence names its binding, and a recorded fixture says nothing of the original](0200-evidence-names-its-binding-and-a-recorded-fixture-says-nothing-of-the-original.md) | Accepted (design decision) |
| 0207 | [A shadow binding reads beside the verified one, and a person promotes it on a run of exact matches](0207-a-shadow-binding-reads-beside-the-verified-one-and-a-person-promotes-it-on-a-run-of-exact-matches.md) | Accepted (design decision) |
| 0208 | [The first release reaches what the person's machine reaches, and serves on that machine only](0208-the-first-release-reaches-what-the-persons-machine-reaches-and-serves-on-that-machine-only.md) | Accepted (owner decision) |
| 0205 | [A sign-in an agent asks for is carried out by its person on the engine's page](0205-a-sign-in-an-agent-asks-for-is-carried-out-by-its-person-on-the-engines-page.md) | Accepted (design decision) |
| 0206 | [The `page_http` mode writes by PUT, with the page's own token, in an acting tab](0206-the-page-http-mode-writes-by-put-with-the-pages-own-token-in-an-acting-tab.md) | Accepted (design decision) |
| 0211 | [Whether the engine may ever act in a session the person already has](0211-whether-the-engine-may-ever-act-in-a-session-the-person-already-has.md) | Accepted (owner decision, by 0242) |
| 0212 | [Whether an application that offers only pixels is ever reached](0212-whether-an-application-that-offers-only-pixels-is-ever-reached.md) | Accepted (owner decision, by 0242) |
| 0213 | [Whether a target someone else wrote may be loaded, and found](0213-whether-a-target-someone-else-wrote-may-be-loaded-and-found.md) | Accepted (owner decision, by 0242) |
| 0214 | [Ways through the refusals that keep a person from their own machine](0214-ways-through-the-refusals-that-keep-a-person-from-their-own-machine.md) | Proposed (design decision) |
| 0215 | [Whether a person may pre-approve an action that leaves nothing to read back](0215-whether-a-person-may-pre-approve-an-action-that-leaves-nothing-to-read-back.md) | Accepted (owner decision, by 0242) |
| 0209 | [The desktop pack reads Windows applications through UI Automation, on a thread of its own](0209-the-desktop-pack-reads-windows-applications-through-ui-automation-on-a-thread-of-its-own.md) | Accepted (design decision) |
| 0216 | [The command pack on three systems: the check on Windows, the administrators' group, and a script the target keeps](0216-the-command-pack-on-three-systems-the-administrators-group-and-a-script-the-target-keeps.md) | Accepted (design decision) |
| 0217 | [The HTTP pack reads answers that are not JSON](0217-the-http-pack-reads-answers-that-are-not-json.md) | Accepted (design decision) |
| 0222 | [The product is aware of the network its machine is on](0222-the-product-is-aware-of-the-network-its-machine-is-on.md) | Accepted (owner decision) |
| 0230 | [The scope is written down on both sides, and it describes without ruling](0230-the-scope-is-written-down-on-both-sides-and-describes-without-ruling.md) | Accepted (owner decision) |
| 0218 | [The desktop pack writes on Linux through a second closed list, and reads back what is stored](0218-the-desktop-pack-writes-on-linux-through-a-second-closed-list-and-reads-back-what-is-stored.md) | Accepted (design decision) |
| 0231 | [The owner's direction is written in exact words of the agent's own](0231-the-owners-direction-is-written-in-exact-words-of-the-agents-own.md) | Accepted (owner decision) |
| 0220 | [A first run on Linux needs no administrator's file](0220-a-first-run-on-linux-needs-no-administrators-file.md) | Accepted (design decision) |
| 0223 | [A network is a place, a recognized network, and a reach](0223-a-network-is-a-place-a-recognized-network-and-a-reach.md) | Proposed (design decision) |
| 0224 | [Whether many-host or many-port looking exists in the first design](0224-whether-many-host-or-many-port-looking-exists-in-the-first-design.md) | Accepted (owner decision, by 0242) |
| 0225 | [Whether anything is ever sent on a network the person has not named](0225-whether-anything-is-ever-sent-on-a-network-the-person-has-not-named.md) | Accepted (owner decision, by 0242) |
| 0226 | [Whether and how long findings about a network are kept](0226-whether-and-how-long-findings-about-a-network-are-kept.md) | Accepted (owner decision, by 0242) |
| 0232 | [Whether a credential the machine holds for the person may be used through its holder](0232-whether-a-credential-the-machine-holds-for-the-person-may-be-used-through-its-holder.md) | Accepted (owner decision, by 0242) |
| 0233 | [Whether being told by another machine is serving it](0233-whether-being-told-by-another-machine-is-serving-it.md) | Accepted (owner decision, by 0242) |
| 0234 | [A watch is a read repeated on a schedule a person confirmed, and a change is reported, never acted on](0234-a-watch-is-a-read-repeated-on-a-schedule-a-person-confirmed-and-a-change-is-reported-never-acted-on.md) | Accepted (by 0247, which builds it) |
| 0236 | [A declared setting whose feature is not built is refused by name](0236-a-declared-setting-whose-feature-is-not-built-is-refused-by-name.md) | Accepted (design decision) |
| 0237 | [One statement of what is said at which scope, by which means, and by whom](0237-one-statement-of-what-is-said-at-which-scope-by-which-means-and-by-whom.md) | Accepted (design decision) |
| 0238 | [Whether a person's own code may take part in proving a write](0238-whether-a-persons-own-code-may-take-part-in-proving-a-write.md) | Accepted (owner decision, by 0242) |
| 0239 | [Whether a person may replace how the product names itself to a system](0239-whether-a-person-may-replace-how-the-product-names-itself-to-a-system.md) | Accepted (owner decision, by 0242) |
| 0219 | [Shadow bindings reach the product, and what a first run by hand found is mended](0219-shadow-bindings-reach-the-product-and-the-first-hour-is-mended.md) | Accepted (design decision) |
| 0221 | [The repository is cleaned, and its names say what each thing holds](0221-the-repository-is-cleaned-and-its-names-say-what-each-thing-holds.md) | Accepted (design decision) |
| 0242 | [The owner takes the recommended answer to every open question](0242-the-owner-takes-the-recommended-answer-to-every-open-question.md) | Accepted (owner decision) |
| 0240 | [One typed reading of where an address is, and one question of a plain link](0240-one-typed-reading-of-where-an-address-is-and-one-question-of-a-plain-link.md) | Accepted (design decision) |
| 0243 | [The machine reads its own network state, and sends nothing](0243-the-machine-reads-its-own-network-state-and-sends-nothing.md) | Accepted (design decision) |
| 0247 | [A watch keeps only digests, and its due times come from the store's clock](0247-a-watch-keeps-only-digests-and-its-due-times-come-from-the-stores-clock.md) | Accepted (design decision) |
| 0241 | [Protected settings loosen only from an administrator's file, and every key says whether it is built](0241-protected-settings-loosen-only-from-an-administrators-file-and-every-key-says-whether-it-is-built.md) | Accepted (design decision) |
| 0244 | [The desktop pack reads macOS applications, and every act proves the record again](0244-the-desktop-pack-reads-macos-applications-and-proves-the-record-before-every-act.md) | Accepted (design decision) |
| 0246 | [The file pack reads folders, settings files, and growing files, and writes the first two](0246-the-file-pack-reads-folders-settings-files-and-growing-files.md) | Accepted (design decision) |
| 0245 | [A web application is signed in to through its own sign-in page, with a credential stored for the person](0245-a-web-application-is-signed-in-to-through-its-own-sign-in-page-with-a-credential-stored-for-the-person.md) | Accepted (design decision) |
| 0253 | [The key derivation is compiled for speed in every build](0253-the-key-derivation-is-compiled-for-speed-in-every-build.md) | Accepted (design decision) |
| 0248 | [A sign-in goes only where a published version says, and a credential belongs to its target](0248-a-sign-in-goes-only-where-a-published-version-says-and-a-credential-belongs-to-its-target.md) | Accepted (design decision) |
| 0249 | [The HTTP pack reads in bounded work and sends nothing that may write outside a live run](0249-the-http-pack-reads-in-bounded-work-and-sends-nothing-that-may-write-outside-a-live-run.md) | Accepted (design decision) |
| 0250 | [One check of who may change a program, and the MCP pack's local server on Windows](0250-one-check-of-who-may-change-a-program-and-the-mcp-packs-local-server-on-windows.md) | Accepted (design decision) |
| 0251 | [The evidence for promoting a shadow binding says what it covers, and a shadow costs what it says](0251-the-evidence-for-promoting-a-shadow-binding-says-what-it-covers-and-a-shadow-costs-what-it-says.md) | Accepted (design decision) |
| 0252 | [A record that is one fixed place, a review summary that shows what a tool runs, and a sign-in declared before its probe](0252-a-fixed-place-a-review-that-shows-what-runs-and-a-sign-in-before-its-probe.md) | Accepted (design decision) |
| 0255 | [The file pack discovers and drafts folders, settings files, and growing files as reads](0255-the-file-pack-discovers-and-drafts-folders-settings-files-and-growing-files-as-reads.md) | Accepted (design decision) |
| 0254 | [A person signs in in a window of the engine's own browser, and the web pack stops on every declared sign of expiry](0254-a-person-signs-in-in-a-window-of-the-engines-own-browser-and-the-web-pack-stops-on-every-declared-sign-of-expiry.md) | Accepted (design decision) |
| 0261 | [A proxy value's credentials are removed before any other cut, and the network readers are bounded and read only the kernel](0261-a-proxy-values-credentials-are-removed-before-any-other-cut.md) | Accepted (design decision) |
| 0276 | [What the scout may learn from a program itself, and by which means](0276-what-the-scout-may-learn-from-a-program-itself-and-by-which-means.md) | Accepted (design decision) |
| 0262 | [Watches are held to their end, their budget, and their run's own result](0262-watches-are-held-to-their-end-their-budget-and-their-runs-own-result.md) | Accepted (design decision) |
| 0269 | [A stored credential's attempts outlive a restart, an answer nobody can read is an attempt, and sign-ins without the person are limited](0269-a-stored-credentials-attempts-outlive-a-restart-and-sign-ins-without-the-person-are-limited.md) | Accepted (design decision) |
| 0277 | [The North Star: an agent uses the software a person already has, without waiting for its maker](0277-the-north-star-an-agent-uses-the-software-a-person-already-has.md) | Accepted (owner decision) |
| 0258 | [Every crate declares its failure codes in one table, with a meaning, a way out, and whether it passes by itself](0258-every-crate-declares-its-failure-codes-in-one-table.md) | Accepted (design decision) |
| 0270 | [The HTTP pack sends an asserted read only from a published version, and bounds what it leaves running](0270-the-http-pack-sends-an-asserted-read-only-from-a-published-version-and-bounds-what-it-leaves-running.md) | Accepted (design decision) |
| 0271 | [The check of programs the product starts, after a second reader](0271-the-check-of-programs-the-product-starts-after-a-second-reader.md) | Accepted (design decision) |
| 0266 | [The engine's log says each start and each stop and what asked for it, and `engine logs` reads it where the engine runs](0266-the-engines-log-says-each-start-and-stop-and-what-asked-and-engine-logs-reads-it-where-the-engine-runs.md) | Accepted (design decision) |
| 0273 | [The settings keeper, and the registry's owed lines, after a second reader](0273-the-settings-keeper-and-the-registrys-owed-lines-after-a-second-reader.md) | Accepted (design decision) |
| 0263 | [A person hands the scout files of the project's sources folder, by names the discoverers declare](0263-a-person-hands-the-scout-files-of-the-projects-sources-folder-by-names-the-discoverers-declare.md) | Accepted (design decision) |
| 0272 | [The file pack's folder writes act through a held folder, after a second reader](0272-the-file-packs-folder-writes-act-through-a-held-folder-after-a-second-reader.md) | Accepted (design decision) |
| 0260 | [The machine's network facts are served as reads of the local user, and one of them is listed](0260-the-machines-network-facts-are-served-as-reads-of-the-local-user-one-listed.md) | Accepted (design decision) |
| 0278 | [What the first runs on Windows and macOS showed about the desktop pack and the check of programs](0278-what-the-first-runs-on-windows-and-macos-showed-about-the-desktop-pack-and-the-check-of-programs.md) | Accepted (design decision) |
| 0279 | [What the three surveys left open against decision 0276 is settled](0279-what-the-three-surveys-left-open-against-decision-0276-is-settled.md) | Accepted (design decision) |
| 0280 | [The `use` toolset lists at most twelve tools](0280-the-use-toolset-lists-at-most-twelve-tools.md) | Accepted (design decision) |
| 0281 | [An MCP result is to carry its decision, so that a call at the MCP door can be explained](0281-an-mcp-result-is-to-carry-its-decision-so-that-a-call-at-the-mcp-door-can-be-explained.md) | Accepted (design decision) |
| 0282 | [Whether the command pack may start programs that run what their arguments name](0282-whether-the-command-pack-may-start-programs-that-run-what-their-arguments-name.md) | Proposed (design decision) |
| 0283 | [Watches reach an agent through the registry, and their person confirms each on the page](0283-watches-reach-an-agent-through-the-registry-and-their-person-confirms-each-on-the-page.md) | Accepted (design decision) |
| 0264 | [The publish page renders the typed review, sign-ins first, from one keeper call](0264-the-publish-page-renders-the-typed-review-sign-ins-first-from-one-keeper-call.md) | Accepted (design decision) |
| 0284 | [The command pack's scout reads a fish completion file and a manual page it is handed](0284-the-command-packs-scout-reads-a-fish-completion-file-and-a-manual-page-it-is-handed.md) | Accepted (design decision) |
| 0285 | [Two watches of the same read for one person share a run, and ending one never cancels a run another rests on](0285-two-watches-of-the-same-read-for-one-person-share-a-run-and-ending-one-never-cancels-a-run-another-rests-on.md) | Accepted (design decision) |
| 0286 | [The macOS walk lists a title bar's buttons and walks nothing under them](0286-the-macos-walk-lists-a-title-bars-buttons-and-walks-nothing-under-them.md) | Accepted (design decision) |
| 0287 | [The last check under way on a run a check opened cancels it](0287-the-last-check-under-way-on-a-run-a-check-opened-cancels-it.md) | Accepted (design decision) |
| 0288 | [Whether a page shows a sign now is the page's own answer, and a slow answer is never a no](0288-whether-a-page-shows-a-sign-now-is-the-pages-own-answer-and-a-slow-answer-is-never-a-no.md) | Accepted (design decision) |
| 0290 | [The command pack's scout reads a PowerShell help file and a module manifest as data](0290-the-command-packs-scout-reads-a-powershell-help-file-and-a-module-manifest-as-data.md) | Accepted (design decision) |
| 0289 | [A failure is made from its code's row, so the place that makes it cannot give it another class](0289-a-failure-is-made-from-its-codes-row.md) | Accepted (design decision) |
| 0291 | [A watch ends when its condition holds on a check's own result](0291-a-watch-ends-when-its-condition-holds-on-a-checks-own-result.md) | Accepted (design decision) |
| 0292 | [A web tab sends only to its surface and the destinations its tool declares, and the publish page says so](0292-a-web-tab-sends-only-to-its-surface-and-the-destinations-its-tool-declares.md) | Accepted (design decision) |
| 0293 | [Existing software that we run ourselves is tested against during the build, and may be written to](0293-software-we-run-ourselves-is-tested-against-during-the-build-and-may-be-written-to.md) | Accepted (owner decision) |
| 0294 | [The owner hands the open questions, and the deciding, to the agents](0294-the-owner-hands-the-open-questions-to-the-agents.md) | Accepted (owner decision) |
| 0295 | [A field test is run from a recipe kept outside the repository, by a runner that names no software](0295-a-field-test-is-run-from-a-recipe-kept-outside-the-repository.md) | Accepted (design decision) |
| 0296 | [The build is ordered by what the next field test needs, in eight stages](0296-the-build-is-ordered-by-what-the-next-field-test-needs.md) | Accepted (design decision) |
| 0297 | [A property a description does not require is drafted as possibly null](0297-a-property-a-description-does-not-require-is-drafted-as-possibly-null.md) | Accepted (design decision) |
| 0298 | [A draft with an open question waits in a holding folder, and the target compiles whole](0298-a-draft-with-an-open-question-waits-in-a-holding-folder.md) | Accepted (design decision) |
| 0299 | [Programs that run what their arguments name are reached in two steps, and a server started through a launcher starts with its file pinned](0299-programs-that-run-what-their-arguments-name-are-reached-in-two-steps.md) | Accepted (design decision; decides 0282) |
| 0300 | [Publishing always needs a person, and the short path to a working read is the sandbox](0300-publishing-always-needs-a-person-and-the-short-path-is-the-sandbox.md) | Accepted (design decision) |
| 0301 | [Every part has a proper name, and a description is not a name](0301-every-part-has-a-proper-name.md) | Accepted (owner decision) |
| 0302 | [A field named as a secret is never drafted as an input or an output](0302-a-field-named-as-a-secret-is-never-drafted-as-an-input-or-an-output.md) | Accepted (design decision) |
| 0303 | [A declared sign-in is kept, and a read that waits for one is reported as waiting](0303-a-declared-sign-in-is-kept-and-a-read-that-waits-for-one-is-reported-as-waiting.md) | Accepted (design decision) |
| 0304 | [A field build stands in for the person in a field test](0304-a-field-build-stands-in-for-the-person-in-a-field-test.md) | Superseded by 0310 |
| 0305 | [An output that may be absent is declared so, in the plan and in the output schema](0305-an-output-that-may-be-absent-is-declared-so-in-the-plan-and-the-output-schema.md) | Accepted (design decision) |
| 0306 | [The short path is `add`, then a trial, and only a built action is listed up front](0306-the-short-path-is-add-then-a-trial-and-only-a-built-action-is-listed-up-front.md) | Accepted (design decision) |
| 0307 | [What the evidence level is for, and what a review shows about a tool's evidence](0307-what-the-evidence-level-is-for-and-what-a-review-shows-about-a-tools-evidence.md) | Accepted (design decision); point 2 superseded by 0311 |
| 0308 | [The first capture records reads only, in the engine's own browser, and drafts to the holding folder](0308-the-first-capture-records-reads-only-in-the-engines-own-browser-and-drafts-to-the-holding-folder.md) | Accepted (design decision) |
| 0309 | [Two disagreements between documents are settled: the two folders named `sources`, and what guard checks](0309-two-names-are-settled-the-source-folder-and-what-guard-checks.md) | Accepted (design decision) |
| 0310 | [The field harness runs in a closed network, and says in the ledger what it is](0310-the-field-harness-runs-in-a-closed-network-and-says-in-the-ledger-what-it-is.md) | Accepted (design decision) |
| 0311 | [The review shows, from the ledger, what this engine proved](0311-the-review-shows-from-the-ledger-what-this-engine-proved.md) | Accepted (design decision) |
| 0312 | [A kind of step is marked as never, as declared, or as able to change the system](0312-a-kind-of-step-is-marked-never-as-declared-or-can-change-the-system.md) | Accepted (design decision) |
