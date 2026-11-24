# First-release effort review, 2026-11-24

One reading of Stage 6 of [the Build Order](../plan/build-order.md), the first release, against the second measure of the North Star: how little the person must do. It asks what a person, and the agent working for them, does between an install and a tool an agent calls, and whether each step is one somebody decides or one the product could have taken. It read `crates/anything2mcp/tests/first_hour.rs`, the shortcuts in `crates/a2m-actions/src/faces.rs`, the action catalog, and decisions 0296, 0306, 0392, and 0397. Nothing was run and nobody was timed.

## What a person and their agent do today

For a system that publishes a description and needs no sign-in, to one published read:

| Step | Who | Decides something |
| --- | --- | --- |
| Install | The person | Yes, once |
| `init` | Either | No |
| `add` with an address and a kind | Either | The address |
| `draft delete` for each draft not wanted | The agent | Which tools to keep |
| `fixture record` with the tool and an input | The agent | No: the scout's report carries the input |
| `binding promote` with the tool and a mode | The agent | No: it is the first binding's mode |
| `test` | The agent | No |
| `publish` | The agent | That it is time to ask |
| Read the page and pass the presence prompt | The person | Yes: this is the review |
| `connect` for their client | Either | Which client |

For a web application with a sign-in and no description, once packages E2 and E7 are built, the person also signs in in the engine's window, begins a capture and uses the application, and answers the question of each held draft. Each of those is theirs by design.

## Findings

| # | Finding | Where | Serious | Kind | Sorted to |
| --- | --- | --- | --- | --- | --- |
| 1 | Between a read the scout already proved and a publish lie two commands for each read and two for the target, none of which decides anything. Twenty reads are forty-two commands. Decision 0296 aimed at about three commands for a read-only tool, and decision 0306 reached that only as far as a trial | Stage 2, section 404 | High | Weaker than it needs to be | [Decision 0950](../decisions/0950-the-steps-between-a-proven-read-and-a-publish-are-one-action.md): one action, `draft.ready`; package S3 |
| 2 | The bar's number, a first tool in under ten minutes for an agent with no prior knowledge, has never been taken. The test that stands for it runs in six seconds of machine time with every command already known | Section 107, H-18 | High | Unmeasured | Package 5.6a, with the field harness's prompt standing for the person and said to be so; again at the owner's sitting |
| 3 | On the web path a person may meet the presence prompt three times before the first tool: at the sign-in, at the start of a capture, and at the publish. Each is right alone. Nobody has counted them together or timed them | Decisions 0254, 0392; H-12 | Medium | Unmeasured | FT6 counts what a person does (H-52). No change now |
| 4 | Fetching a browser (`browser.install`) waits until after the release, so on a machine with no browser the product can use, the first hour stops. The stop names what is missing; nothing says in one place what a person installs first | Decision 0397; section 605 | Low | Gap in the documents | The guidance file's first page, in package 5.3, whose test now has a reason to hold it |
| 5 | Stage 6's table still lists 1.7 first and has no row for 5.6a, S3, 9.2a, or the maps of 9.3 | The Build Order | Low | Out of date | Fixed in this round |

## What was not looked at

The install itself, which is the owner's and unpublished. The words of the built-in pages, which package 5.3 tests. The approval of a write, which belongs to Stage 4.
