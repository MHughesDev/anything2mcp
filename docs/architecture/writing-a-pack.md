# Writing a pack

How a developer, a person or an agent, adds support for a new kind of system. It is drawn from the nine packs in this repository, not from intentions. The model used throughout is the file pack, `crates/a2m-pack-file`: it has every part a pack has, it is the smallest pack with a manifest at the crate root, and its stand, `targets/example-files`, is a library with no server, so the whole pack can be read in one sitting. Where another pack shows something better, the text says so.

Nothing here changes a rule. The rules are in [AGENTS.md](../../AGENTS.md); the bar a pack must clear is in [section 202](../spec/2-kernel/202-kernel-and-packs.md).

## What a pack is, and is not

A pack is a Rust crate in this repository that knows one kind of system and hands the kernel six things through the provider contract ([section 203](../spec/2-kernel/203-the-provider-contract.md)): schemas for its file kinds, a lowering from verbs to steps, lint rules, a driver, discoverers, and drafting rules. Knowledge about a kind of system lives in its pack and nowhere else.

It is not a plug-in. A plug-in is a separate process in any language that speaks a plug-in protocol; that protocol is designed and not built (see the last section).

What a pack may depend on, from the layering table in the [technical reference](../reference/technical-reference.md#crate-layering): layers 1 to 5 (`a2m-core` up to `a2m-provider`), and the base of its own pack, never another pack. It cannot call the engine, the scheduler, the action registry, or a front door: it registers what it offers and is called. `crates/a2m-pack-file/Cargo.toml` shows the whole list. Only the product crate names a pack: `crates/anything2mcp/src/packs.rs` has one line for each, and `tools/docs_audit.py` refuses a layering mistake by name.

## The entry bar, and where the file pack clears each item

The eight items of section 202. A pack that cannot clear item 2 for its writes may still be accepted for reads.

| # | Item | Where to see it in the file pack |
| --- | --- | --- |
| 1 | A driver, with no retries inside | `impl Driver for FileDriver` in `src/driver.rs`; `src/save.rs` says "never retried" at the replace |
| 2 | A read-back rule for every write, or the write is `irreversible` | `src/lower.rs`: `update` and `create` lower to a save followed by a `confirm` that reads the file again; `least_effect` makes `delete` `irreversible`. Test `an_update_changes_one_span_of_delimited_text_and_reads_back` in `tests/stand.rs` |
| 3 | An answer for each identity strategy | `identity` in `src/lower.rs`: natural, composite and none are possible; stamped and find say why not |
| 4 | A test stand that reports ground truth | `targets/example-files`, used by `tests/stand.rs` |
| 5 | A discoverer whose findings claim no more than was seen | `src/discover.rs`; the manifest caps its level at `discovered`. Test `discovery_reads_layouts_writes_nothing_and_drafts_only_what_the_file_states` |
| 6 | Failures mapped to the existing classes | `src/codes.rs` and the `[[failure_codes]]` of the manifest. Test `every_failure_code_of_the_pack_is_declared_with_its_one_class` in `tests/pack.rs` |
| 7 | Effects and hazards declared | `least_effect` and the lint rules in `src/lower.rs`; the manifest's `[effects]` roots. The vocabulary is [section 213](../spec/2-kernel/213-effects-hazards-and-consent.md) |
| 8 | A decision record for each new mode | [0102](../decisions/0102-the-file-mode-and-what-the-file-pack-registers.md), then [0103](../decisions/0103-the-file-pack-as-building-it-settled-it.md) for what building it changed |

## The parts of a pack, as they are in the code

**The manifest**, `anything2mcp-provider.toml`, is included by the crate (`MANIFEST` in `src/lib.rs`). The registry refuses anything the pack registers that the manifest does not declare. Its sections, as the file pack and the command pack use them:

- `[provider]`: name, version, publisher, `kind = "pack"`.
- `[[modes]]`: the mode's name, archetype, verbs, the step kinds, and which of them are `commit_steps`; its rank, its intent window, and the key it is `exclusive` on.
- `[[surface_kinds]]`, `[[file_kinds]]`, `[[finding_kinds]]`: the words the pack adds. A file kind has one owning pack per surface kind ([decision 0062](../decisions/0062-words-that-arrive-with-packs-are-registered-names-and-packs-share-data-never-code.md)); the web and desktop packs each own `screens/` for their own surfaces ([0203](../decisions/0203-two-packs-may-each-draft-a-file-kind-of-one-name-for-their-own-surfaces.md)).
- `[points]`, `[[discoverers]]`, `[[drafting_rules]]`: what it observes, with a bound on time and bytes and a ceiling on the evidence level, and which findings become which files.
- `[[lint_rules]]` with stable codes; `[[failure_codes]]`, each with exactly one class.
- `[effects]`: the roots it may read and write.
- `[[settings]]`: each with a type, default, bounds, and a doc line. A setting that could loosen a safety limit is `protected = true` and states which direction is `safer`; see `admin_groups` in `crates/a2m-pack-command/anything2mcp-provider.toml`. The stream pack keeps its manifest at `src/manifest.toml`; the others at the crate root.

**The registration**, `Pack::register` in `src/lib.rs`: the driver for the mode, then one `CompilerRegistrations` value that carries the surface kind with its schema, the file kind, the lowering, the drafting rules, and the lint rules; then the discoverer with its describer.

**The driver** has four methods: `prepare` (acquire what the binding needs), `act` (carry out exactly one step), `health`, `release`. The steps of a write are the same in every pack: prepare, act (the commit step is marked), confirm, read back. In the file pack: `open`, `locate`, `set`, `save`, then `confirm`. The driver never sees a path it resolved itself: a surface's folder is a `CheckedPath` made from roots the manifest declares.

**Failures** are `StepFailure` values made with `not_sent`, `answered`, or `unknown`, each carrying a `Failure` with one class and one code. The class alone decides what the engine does next. What the system said goes in with `with_said`, apart from the pack's own sentence.

**The discoverer and the drafting rules.** A discoverer reads and cannot write; the types see to that. A drafting rule turns a kind of finding into a kind of file, and may draft only what the findings state. `src/draft.rs` marks everything the file said as untrusted text. A rule never drafts a write ([decision 0200](../decisions/0200-evidence-names-its-binding-and-a-recorded-fixture-says-nothing-of-the-original.md) holds a binding written by hand to `draft`).

**Lint rules** are registered with codes in the manifest and made in `src/lower.rs`. They report problems in the pack's own files, with a file, a line, and a fix. The cross-system lint matters here: `cargo clippy --workspace -- -D warnings` runs on all three systems in CI, and code that only one system builds has failed it before ([decision 0204](../decisions/0204-one-plain-form-of-a-path-a-pipe-that-is-waited-for-and-a-socket-path-that-fits.md)).

**The stand** is a crate under `targets/`, named `example-<kind>`. It imitates the part of its kind that makes the kind hard, reports what it stored through a control interface, and takes faults as settings. See [the test application plan](../plan/test-application.md) and each stand's README. It names no real product ([rule 9](../../AGENTS.md)).

## The rules a pack is held to

- **No retries inside a driver.** The engine alone decides whether a step is tried again, from the failure's class; a driver that retries hides a second send.
- **Every failure says whether anything was sent.** A write whose fate is unknown must be reconciled before it is repeated; "when in doubt, `Unknown`".
- **A read never runs where a write could.** A read step takes the observing posture; a step that writes must be marked as committing and runs only after the ledger's intent token ([decision 0195](../decisions/0195-a-read-never-runs-where-the-page-may-write-and-a-page-keeps-only-its-structure.md)).
- **Closed lists for anything that acts.** The desktop pack has two: `READ_MEMBERS`, which only asks, and `ACT_MEMBERS`, which sets text and does an action, sent by one function. A test holds that no member is on both lists (`crates/a2m-pack-desktop/src/backend/linux.rs`, [0218](../decisions/0218-the-desktop-pack-writes-on-linux-through-a-second-closed-list-and-reads-back-what-is-stored.md)).
- **Text from a system is untrusted data.** It is kept apart from the pack's sentences and never becomes an instruction or a fix ([section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md)).
- **Secrets go through the redactor.** A credential reaches the driver only through the session lease; none appears in a value, a capture, or a failure (rule 4).
- **Knowledge of a kind of system lives only in its pack.** Locators, endpoints, units and business steps stay in the files that rule 8 names.
- **Nothing guesses.** A shape, a variant or a place with no rule is a named failure (rule 7), never a best effort.

## The order of work the existing packs followed

1. A decision record for the mode, and the design as a spec section and a crate design.
2. The stand first, with its control interface and its faults, and its own tests.
3. Reads: the manifest, the registration, the lowering, the driver's read steps, a test per read against the stand.
4. One verified write, proven by asking the stand what it stored, never by the pack's own read path.
5. Faults: for anything that writes, an expired session, a lost connection after the write was sent, bad input, a stale record, a blocked surface. The file pack's examples: `a_save_stopped_half_way_leaves_the_old_file_and_resumes_to_the_new_one`, `a_file_changed_after_it_was_read_is_stale_and_is_not_replaced`.
6. Drafting: a discoverer and rules, so that a scout ends with reads that work ([decision 0179](../decisions/0179-what-each-pack-drafts-so-that-a-scout-ends-with-reads-that-work.md)).
7. The audit: a reader who did not build the pack reads it against the documents and the ten rules. Every pack's audit found faults its own tests passed ([0138](../decisions/0138-the-vault-and-the-session-keeper-after-their-safety-audit.md) to [0153](../decisions/0153-what-the-compiler-the-plan-and-the-redactor-hold-by-themselves-after-the-safety-audit.md); the file pack's is [0149](../decisions/0149-the-file-pack-after-its-safety-audit.md)).
8. The pack joins the product in `crates/anything2mcp/src/packs.rs`, with a product test, and the changelog and roadmap say so.

## What a pack owes before it is accepted

- Unit tests, and tests against its stand that ask the stand what it stored.
- Fault-injection tests for every write.
- Format, lint, build and tests green on Windows, macOS and Linux (`.github/workflows/ci.yml`).
- Its manifest's failure codes each declared once, held by a test, and every public interface in the technical reference.
- A safety audit by a reader who did not write it, and its findings fixed or recorded.
- A decision record for each mode, and the spec section and crate design true to the code.

## What is not possible yet

**A pack outside this repository: Not yet.** The plug-in protocol that would let a pack live elsewhere, in any language, is designed ([section 202](../spec/2-kernel/202-kernel-and-packs.md), "Packs and plug-ins") and not built. Today a new kind of system is a crate in this repository, compiled into the one binary, and switched on by a setting. A template crate is a package of the [adoption plan](../plan/adoption.md) (AD6) and does not exist; copy the file pack.
