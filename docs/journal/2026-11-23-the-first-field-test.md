# 2026-11-23: the first field test

The entry before this is [the wider reach](2026-11-18-the-wider-reach.md). Between the two, nothing was built: from 2026-11-19 the owner asked for planning, learning, and review only, and that ended today.

## State at the end

- **On `main`:** pull request 19 (merge commit 6f17119), unchanged since the last entry.
- **This session's change:** one runner in `tools/`, three decision records, the field test results, and the documents they change. No crate changed.
- **Stopped:** no system and no engine was left running by a field test. The hourly review stays switched off.

## What the owner decided

- **Existing software that we run ourselves is tested against during the build, and may be written to** ([0293](../decisions/0293-software-we-run-ourselves-is-tested-against-during-the-build-and-may-be-written-to.md)).
- **The open questions, and the deciding, are the agents'** ([0294](../decisions/0294-the-owner-hands-the-open-questions-to-the-agents.md)). The ten rules, the three things an agent does not do alone, and publishing stay as they were: that part is my inference and is written as one.
- **Other agents run on the cheapest model.** The lead does what needs judgment.
- The owner also agreed to five things the lead proposed, which are not yet records because nothing has been built for them: add no new kind of system for now; build the capture of a person's use of a web application; make a read-only tool take about three commands; propose a lighter publish for a read-only tool; and try reads against real software, which this session began.

## Done

- **The runner,** `tools/field_test.py` ([0295](../decisions/0295-a-field-test-is-run-from-a-recipe-kept-outside-the-repository.md)): it starts one system from an image pinned by digest, runs a recipe's steps, reads the system's own store, and writes a report. A known fault is a step marked `gap`.
- **Field test 1,** reads of an HTTP service with a published description: [the results](../reviews/field-tests.md). The recipe ran three times with the same result, all 20 steps as it expects.

## Measured

Of three drafted reads tried against existing software, one returned its value and two stopped. Of 39 drafts the scout wrote, none compiled until each endpoint was edited by hand.

## Learned

- **The stands could not have shown this.** Every finding is something the project did not think to put in a stand: an older form of description, a description handed in as a file, a doubled name in a large description, a field whose name looks like a secret's, and an optional object answered as `null`.
- **Finding 7 is the one that matters most.** A read that stops whenever a described object is absent will stop on most real services. Rule 7 is not the fault; the question is what counts as unknown. It needs a decision record before code.
- **This session's machine can run existing software itself.** A container engine is installed and images can be fetched, with no change to the environment. Fetching a release file directly from its maker's site is refused by the network policy.
- **A deep folder stops the engine** with `socket_path_too_long`. The runner takes `--home` for that.

## Noticed, not acted on

- `draft.lint` answers `ok: true` while it lists error-level problems, and `draft.try` then refuses the same target with `lint_failed`.
- A field test runs only where a container engine runs. On the hosted Windows and macOS runners it does not.
- Recipes have no lasting home. They are in the lead's working folder and were handed to the owner as files.

## Decided without the owner having read it

0295.

## Next

1. **F3:** decide what a read does when a described object is absent, with a record, then build it, first as a case in the test application.
2. **F2:** close findings 1, 2, 4, 5, and 6, each first as a case in the test application. The recipe's `gap` steps show when each is closed.
3. **F4:** the proxy that makes faults on purpose, and the first verified write of a field test.
4. Decision 0282 (whether a path may start an interpreter) is now the lead's to decide under 0294. It decides whether most existing MCP servers can be started at all.
5. The order of the last entry (P3b, the answer lost on macOS without TLS, W2d, X1, X5, P2) stands behind these.
