# Adoption: being found, tried, and trusted

The plan for getting Anything2MCP into the hands of software engineers, written on 2026-11-18 for the owner and for later agents to act on. It rests on the survey in [research note 27](../research/27-the-public-landscape-of-agent-tools.md), on what the repository holds today, and on a first run of the README's try-it commands by hand. It changes no rule and no spec section. Each work package below says what proves it and whose act it is; packages that need the code follow the [working method](working-method.md) and enter the [roadmap](roadmap.md) when an agent takes one up.

## 1. What the survey shows

Note 27's findings under "What makes the popular ones popular", read from the pages of about 45 projects:

1. **A first result in one line, with no authoring.** The most-used browser and reference servers start with one command from a package runner (`npx …`, `uvx …`), and the first command gives a result.
2. **The package manager a person already has.** Most use `npx`, several `uvx`, `uv`, or `pip`; one each `cargo` and `brew`.
3. **The README leads with the client configuration** and a one-line description, and names the agents it works with, with none preferred.
4. **A tool to see the tools.** The MCP Inspector is one command, and it is how people check a server before trusting it.
5. **Frequent small releases.** The projects with the most stars all published within the two weeks before the survey.
6. **A registry and a list.** The public MCP Registry checks who owns a namespace; the curated lists are where most discovery starts.
7. **A place for the community to add.** Plug-ins, pieces, and components that other people contribute.
8. **Candour about limits.** The more trusted READMEs say plainly that their own safety switch is "not a security boundary".

Its findings under "What this product has that they do not" add what this project has that the pages read do not show: no model at run time as a rule, a write recorded before it is made and read back after, a person who approves and an agent who cannot, secrets kept from the caller by a wrapper type, and nothing that phones home. Its caution governs this whole plan: the headline sentence "should not be said before it is true".

## 2. Where this project stands against each

| What the survey shows | Here, on 2026-11-18 |
| --- | --- |
| One-line first result | **No.** Nothing is published, so no package runner can fetch it. From source, a first read takes a clone, a build, six setup lines, and four commands; a published tool takes a loop and four more commands, and then a person's presence prompt that many machines cannot raise (see "What the first run by hand showed" below) |
| The package manager a person has | **Built, not published.** [`.github/workflows/release.yml`](../../.github/workflows/release.yml) builds eight maturin wheels and a source distribution, and nine npm packages (a launcher and eight `@anything2mcp/<os>-<cpu>` packages) whose binaries are taken from the wheels; it checks each set together and has never published. None of the three names is held ([publishing setup](publishing-setup.md)). crates.io has no job: `publish = false` and the unpublished `a2m-*` crates block it, and a decision is needed. `bindings/python` and `bindings/npm` hold the packages and thin clients; the `bindings` CI job builds and tests them on three systems |
| README leads with client configuration | **Now partly.** The README gives the generic stdio block for `anything2mcp bridge`. `connect` (the action `client.connect`) is in the catalog and answers `action_not_built` |
| A tool to see the tools | **Partly.** `tool search --full` and `tool describe` give a published tool's name, version, effect, hazard, plan hash, schemas, and annotations; `plan show` prints a compiled plan. Nothing shows a binding's evidence level or the gates a version passed, after the publish page closes |
| Frequent small releases | **None yet.** [CHANGELOG.md](../../CHANGELOG.md) has only "Unreleased", and its first line, "Nothing usable yet", is out of date. [RELEASING.md](../../RELEASING.md) has the steps |
| A registry listing | **None.** Needs a published package first |
| A place for the community to add | **Designed, not built.** The plug-in protocol is designed in [section 202](../spec/2-kernel/202-kernel-and-packs.md), [section 203](../spec/2-kernel/203-the-provider-contract.md), and [section 204](../spec/2-kernel/204-supervising-subsystems.md); `plugin.install` and `plugin.list` answer `action_not_built`, and the framing a plug-in speaks is not built. A first-party pack can be contributed today as Rust crates |
| Candour about limits | **Now in the README**: what it does not do, what is not built, and where a run by hand stops |

### What the first run by hand showed

These were found by running the README's commands in an empty folder with the binary from `cargo build -p anything2mcp`. Each is product work, not documentation work.

1. **On Linux the vault needs an installation-level file.** `vault.backend` may be set only at the installation layer, which is `/etc/anything2mcp/settings.toml` unless `A2M_HOME` is set; the user and environment layers are refused with `setting_protected`. With no file, every command after `init` fails with `engine_degraded`. A newcomer must either write to `/etc` as an administrator or set `A2M_HOME` and the passphrase variable.
2. **`init` starts an engine that cannot open its vault**, and later commands talk to that engine until it is stopped.
3. **`A2M_HOME` has two traps:** inside the project folder it is refused (`path_not_allowed`), and on a deep path the socket's path is too long (`socket_path_too_long`, which names the limit and is clear).
4. **The publish cannot be finished on a machine without a presence prompt**: a container, a machine reached over SSH, or a Linux desktop whose administrator has not installed the polkit policy file, which no package carries yet. The page says so plainly. The macOS and Windows prompts have not been run by a person.
5. **The scout's terminal output is about 500 lines**; its readable summary (`text`) is last.
6. **Next steps disagree.** After the scout, `target status` and `lint` name `publish.request` and `version.publish` as next while every operation still lacks a test; `test` exits 0 with `passed: false`.
7. **The bridge's own instructions name the wrong field.** They say to call `a2m_action_run` "with its `action` and `args`"; the tool's schema takes `action` and `input`, and `args` is refused.
8. **Removing unwanted drafts takes one command each**, five for the test application.
9. **The publish page lists an environment `production`** that the target file written by `target add` does not declare.

## 3. Work packages, in order

Each package is one branch and one pull request, unless it is the owner's act. "Agent" means any agent working under [AGENTS.md](../../AGENTS.md); "owner" means only the owner.

### AD1. The first publish

| | |
| --- | --- |
| What | Hold the three names by publishing a real pre-alpha version through the release workflow |
| Owner's acts | Choose the version number (0.0.1 is what the workflow and `Cargo.toml` hold). Create the npm organization, publish the nine npm names once from the owner's account, add the trusted publishers on PyPI and npm, create the `pypi` and `npm` environments with required reviewers, and start the workflow by tag or by hand ([publishing setup](publishing-setup.md)). Decide whether the `a2m-*` crates are published, which crates.io needs |
| Agents' acts | Before it: make "Unreleased" in the changelog true, set the version in the three places, run the audit. After it: install by each route in a clean environment, run both command names, mark the names held, and write the journal entry. An agent never handles a token and never starts the release workflow |
| Proof | `pip install anything2mcp` and `npm install -g anything2mcp` in clean environments on the three systems give `anything2mcp --version`; publishing setup marks each name held |

### AD2. One command to a first result

| | |
| --- | --- |
| What | A new action `demo.run`, with its command `demo`, generated from the registry like every command. It starts the built-in test application on a free loopback port, writes a project with the `example` target into a folder it names, publishes a read through the same page a person uses, calls it, then asks for one write to a declared test record, which rests at `needs_approval`, is approved by the person on the page, and is read back. Once AD1 is done, `npx anything2mcp demo`, `uvx anything2mcp demo`, and `pipx run anything2mcp demo` reach it with nothing else installed |
| Needs a decision | Shipping a test stand inside the product binary is new: the test application today is a crate under `targets/` that only tests use. The record must say how its control interface is kept to loopback and to the demo's run, and why its seed data is synthetic |
| Crates | `a2m-actions` (the action), `anything2mcp` (joining the test application and the demo's target files), `example-app` (served from a library call, as the harness already does) |
| What it cannot hide | The approval still needs the presence prompt. On a machine with none, the demo shows the page, says why nothing can be approved, and ends after the read. It must never pass the prompt for the person |
| Proof | A product test, as `first_hour.rs` stands in for the prompt: `demo` from an empty folder reaches one read and one approved write; the test application's control interface reports exactly one write with the values sent; the whole run takes under 60 seconds on all three systems in CI. A second test with no prompt ends after the read with the page's reason |
| Whose | Agent |

### AD3. A tool inspector

| | |
| --- | --- |
| What | One command that lists a project's published tools with, for each: effect class, hazard, the evidence level of each binding that may act, the gates the version passed, the gates that did not run because they are not built, who published it and when, and the plan hash. `tool.search --full` already gives the effect, hazard, plan hash, schemas, and annotations; the rest is held in the store at publish time and shown only on the publish page |
| Choice | Extend `tool.describe` and `tool.search --full` with an `evidence` block, or add a new action. An action's output schema is what every face is generated from, so either is a decision record |
| Crates | `a2m-actions`; where the publish's gate results are kept, `a2m-engine` or `a2m-ledger` |
| Proof | A product test publishes the first hour's read and the inspector shows its binding at `executed`, the two gates passed, and the ten not run, matching the publish page; it shows no secret and no value from a fixture. Separately, by hand and written in the journal: an outside MCP inspector client lists the same tools through `bridge` |
| Whose | Agent |

### AD4. Client configuration written by `connect`

| | |
| --- | --- |
| What | Build `client.connect`. A client profile is data in one file, as [section 222](../spec/2-kernel/222-front-doors-and-the-local-engine.md) says ("a client profile row in a data file … changes presentation only"): where that client keeps its configuration on each system, and the shape of its entry. `connect generic` prints the block the README shows. No profile changes what is allowed or which tools exist; adding a client is adding a row and a test |
| Rules it keeps | It writes no passphrase or other secret into a client's file; it adds or replaces only its own entry and keeps every other server's; it writes full paths; it says what it wrote and where |
| Crates | `a2m-actions` (the handler), `anything2mcp` (the profile file) |
| Proof | For every profile and for `generic`, a test writes into a temporary home, parses the result, finds the entry and every entry that was there before, and runs `bridge` from the written command line to answer `initialize` and `tools/list`. Writing twice gives the same file |
| Note | [Section 404](../spec/4-building-tools/404-setup-and-ease-of-use.md) shows `connect` with profile names of particular clients. When built, the README and the docs name the generic block first and list profiles without ranking them |
| Whose | Agent |

### AD5. A listing in the public MCP registry

| | |
| --- | --- |
| What | List the published server where people look for servers. Note 27 records that the MCP Registry has a publisher command, checks namespace ownership by a GitHub sign-in, DNS, or HTTP, and was in preview with an API freeze at v0.1. It did not read the listing format |
| Needs first | AD1, and a short research brief (agent) that reads the registry's current requirements and writes them up in `docs/research/` |
| Owner's act | Submitting, and proving the namespace, under the owner's account |
| Agents' act | Preparing whatever file the listing needs, in the repository, from the brief |
| Proof | The listing resolves to the published package; the journal records it |

### AD6. The pack contribution path

| | |
| --- | --- |
| What | Make it plain how someone adds a kind of system. Two routes, both under one contract ([section 203](../spec/2-kernel/203-the-provider-contract.md)): a first-party pack, as Rust crates in this repository, which works today; and a plug-in, a separate process in any language speaking the plug-in protocol, signed or off by default, which is designed and not built |
| Parts | (a) A pack template: an `a2m-pack-<kind>` crate with its embedded `anything2mcp-provider.toml`, one driver verb, a drafting rule, and a test stand under `targets/` with a control interface, copied from the smallest existing pack. (b) The bar it must clear: the eight items of [section 202](../spec/2-kernel/202-kernel-and-packs.md) ("The bar a new pack must clear"), including a read-back rule for every write and a decision record for its mode, and the dimensions of [section 107](../spec/1-product/107-the-bar.md). (c) The plug-in protocol itself, when its roadmap package comes, with the same bar |
| What CONTRIBUTING.md should say | Open an issue naming the kind of system and its test stand before writing code; a pack is accepted for reads without item 2; every write needs its read-back and fault tests; no real application is named anywhere (rule 9); the checks that must pass on three systems |
| Proof | (a) A test builds the template as a crate in the workspace and registers it; the audit's layering check passes. (b) and the CONTRIBUTING text: the audit reports no findings |
| Whose | Agent; a new pack's mode decision is a design decision any agent may record |

### AD7. Target files the community writes

Rule 9 says the project publishes no catalog of target files for real applications, and they live outside this repository. That is the owner's rule.

**The open question, as note 27 states it:** "Rule 9: may the project list or link target files that other people publish and host, without hosting them or naming a real application in this repository?" [Decision 0213](../decisions/0213-whether-a-target-someone-else-wrote-may-be-loaded-and-found.md) (Proposed, owner decision) asks it in two parts and recommends yes to loading a target someone else wrote under [section 502](../spec/5-security/502-defending-against-a-manipulated-agent.md)'s rules, and no, for now, to the project pointing at where they are.

What can be done without changing the rule, by an agent:

- **Document the format** for an outside author in one place: [file formats](../reference/file-formats.md) already holds it; add a page on what a target folder holds, what must be synthetic (fixtures), and what must never be in it (a secret, a recorded business value).
- **A convention for publishing one's own**: in the author's own repository, under a name the author chooses; the target files, their offline tests and synthetic fixtures; a statement of what it was proven against and when; and, once section 502's signing is built, a signature. The project neither hosts, lists, nor links these.
- **Nothing in this repository names one.**

Proof: the page exists and the audit passes. Anything beyond this waits for the owner's answer.

### AD8. Release cadence and a changelog people can read

| | |
| --- | --- |
| What | After AD1: a pre-release whenever a milestone's work merges and at least every two weeks while `main` is green, following [RELEASING.md](../../RELEASING.md). The changelog's "Unreleased" says what a user can now do, in one line per change, and loses its out-of-date first line |
| Owner's act | Starting each release, unless the owner gives an agent's access the right to push a version tag, which RELEASING.md allows |
| Agents' act | Keeping "Unreleased" true in every pull request that a user would notice (AGENTS.md already asks this), and preparing each release's section |
| Proof | Registry records show the versions; each version's changelog section names only what a test on that commit proves |

### AD9. A short recorded terminal demonstration

| | |
| --- | --- |
| What | A recording of under two minutes against the test application only: the scout's summary, a trial, the publish page and its presence prompt, a write resting, approved, and read back, and the test application's control interface confirming one write. It names no real application |
| How | The recording is made from a script, and the same script runs in CI with the prompt stood in for, so the recording cannot drift from the product. A tool to record or play it is checked for its licence before use, as a dependency would be |
| Owner's act | The final recording, at a desktop, because the presence prompt needs a person |
| Proof | The script's CI run; the recording in `docs/assets/` |
| Whose | Agent writes the script; owner records |

### AD10. The write-up

An article the owner may publish, under the owner's name, arguing for "no model at run time, every write verified". An agent drafts it in the repository; the owner decides whether and where it appears. It may make only the claims in the table under "What may be said, and when" that are marked "May be said now", each with its test named in a footnote, and it says what is not yet true as plainly as the README does.

## 4. What may be said, and when

| Claim | What proves it today | May be said |
| --- | --- | --- |
| No model runs inside a tool call; a run is a compiled plan | `first_read.rs`: a read five times with one trace signature; `first_hour.rs`: two scouts of the same application give the same report hash | Now |
| A write is recorded before it is made and read back after | `verified_write.rs`: `a_write_rests_is_approved_by_a_person_and_is_written_and_verified_exactly_once` (approval, intent, act, read-back, in that order in the trace; the test application confirms one write); the pack tests of each write-capable pack | Now, for the test stands |
| A lost answer or a killed process does not write twice | `verified_write.rs`: `an_answer_lost_after_the_write_was_sent_is_reconciled_and_not_written_twice`, `a_process_killed_after_the_write_was_sent_leaves_no_duplicate_when_the_run_is_resumed` | Now, for the test stands |
| An agent cannot approve a write or publish a tool | `verified_write.rs` (an agent's approval refused; no command carries the word); `local_engine.rs`: `a_program_that_says_it_is_a_person_at_a_terminal_is_an_agent_and_raises_no_prompt`, `with_no_presence_prompt_a_write_through_the_served_engine_rests_and_its_page_says_why`, `an_agents_publish_waits_on_its_page_and_a_persons_press_publishes_it_once` | Now |
| The person's approval is the operating system's own prompt | The `presence` CI job: polkit on Linux, against a real polkit | Now, for Linux only |
| Secrets never reach the caller | `signed_in_read.rs`: `a_read_behind_a_bearer_token_fails_without_it_reads_with_it_and_rests_when_it_is_revoked`, `a_secret_the_system_itself_returns_is_masked_and_is_in_nothing`; `first_hour_signed_in.rs` | Now, for the signing-in routes built |
| Nothing phones home | `anything2mcp config show`: usage export, crash reports, and update checks are off by default. No test watches the network for it | Now, as "off by default"; not as a tested guarantee |
| Nine kinds of system, one engine | `other_packs.rs`: a read through each pack, through the product; the pack tests | Now, as reads through each pack and writes where the table in the README says |
| Tested on Windows, macOS, and Linux | The `rust` CI job's matrix | Now, for the test suite; not for a person's approval on macOS or Windows |
| From an address to a published read in minutes | `first_hour.rs`, timed under ten minutes, with the prompt stood in for | Only as "in the test"; by hand it needs AD1 and a machine with a prompt |
| One install by pip, npm, or cargo | Nothing yet | After AD1 |
| "Your agent proposes; a person approves; the write is recorded first and read back after; no model runs when a tool does" (note 27) | Shown on the test stands; not shown on any real system | After roadmap package 9.5, with the owner, on real systems |
| Every tool passes twelve publish gates | Two gates are built | Not until the rest are built |
| Faster than driving the page | The first measurement of `page_http` was not faster than the screen read (roadmap 4.2) | Not until measured |

The bar's scorecard in [section 107](../spec/1-product/107-the-bar.md) still marks most rows `designed`. Bringing each row up to what the tests now show, by its own rule ("a row only moves forward on proof"), is agent work and should come before AD10.

## 5. What agents do not do

Agents do not post, announce, or reply in any public place; do not submit to registries, directories, or curated lists; do not open accounts; do not publish a release, start the release workflow, or handle a registry token; and do nothing under the owner's name or account. The owner does those. Agents prepare everything those acts need, in the repository, and say in the journal what is ready.

## 6. How to know it is working

Nothing phones home, so every signal is one the public already sees: download counts on PyPI and npm, stars and forks, issues opened and what they ask, pull requests from outside, and questions that show where people get stuck. An agent may read these public numbers and write them in the journal with their date. A question asked twice is a candidate for the README or a fix; the list in "What the first run by hand showed" is the first such list.
