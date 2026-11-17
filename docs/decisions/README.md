# Decision records

One file per decision: the context, what was decided, what follows from it, what was rejected, and what would reopen it. Add a new numbered file for every decision that changes the design; never rewrite an old one. To reverse a decision, add a new record that supersedes it and mark the old one superseded.

A record's status is one of: **Accepted**, in force; **Proposed**, a change someone wants and has not been given, not to be built on until accepted or supported by a tested hypothesis; **Superseded**, replaced by a later record that names it.

Who may change what ([record 0055](0055-agents-plan-build-and-merge-without-waiting-for-the-owner.md)): a record marked as a design decision can be superseded by any agent with a new record that gives the evidence. A record marked as an owner decision is changed only by the owner; an agent that thinks one is wrong writes a Proposed record and carries on. A record the owner has not read says so in its status line and is in force all the same.

Records 0001 to 0054 were written when the product was named Web2MCP, and keep the names of their time. [Record 0056](0056-one-name-everywhere-anything2mcp.md) has the key from the earlier names to the present ones.

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
| 0033 | [Policy decisions use Cedar, with grants compiled to it](0033-policy-decisions-use-cedar-with-grants-compiled-to-it.md) | Accepted (design decision) |
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
| 0072 | [Expressions are a CEL subset, on the engine's own parser, checker, and evaluator](0072-expressions-are-a-cel-subset-on-the-engines-own-parser-and-evaluator.md) | Accepted (design decision) |
| 0076 | [One trace record per step, and a signature chained over record digests](0076-one-trace-record-per-step-and-a-signature-chained-over-record-digests.md) | Accepted (design decision) |
| 0077 | [Logs through the `tracing` crate with our own subscriber, and spans through a handle](0077-logs-through-the-tracing-crate-with-our-own-subscriber-and-spans-through-a-handle.md) | Accepted (design decision) |
| 0071 | [The MCP server stands on the official SDK behind a seam, and serves two revisions](0071-the-mcp-server-stands-on-the-official-sdk-behind-a-seam-and-serves-two-revisions.md) | Accepted (design decision) |
| 0074 | [The vault's cipher, key stores, and other outside crates](0074-the-vaults-cipher-key-stores-and-other-outside-crates.md) | Accepted (design decision) |
| 0075 | [The vault's interface, as building it settled it](0075-the-vaults-interface-as-building-it-settled-it.md) | Accepted (design decision) |
| 0070 | [The browser engine stands on its own client, over the debugging pipe](0070-the-browser-engine-stands-on-its-own-client-over-the-debugging-pipe.md) | Accepted (design decision) |
| 0073 | [Decimals use operators, division is a function, and how a field type normalizes](0073-decimals-use-operators-division-is-a-function-and-how-a-field-type-normalizes.md) | Accepted (design decision) |
