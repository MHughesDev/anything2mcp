# 0264. The publish page renders the typed review, sign-ins first, from one keeper call

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** the action registry's person act (`crates/a2m-actions/src/acts.rs`), the person surface's act page (`crates/a2m-local/src/surface.rs`), the review summary (`crates/a2m-files/src/review.rs`), the `statement` of four packs' lowerings (`a2m-pack-command`, `a2m-pack-http`, `a2m-pack-mcp`, `a2m-pack-web-ui`), [section 403](../spec/4-building-tools/403-testing-and-publish-gates.md), and the designs of those crates. **Carries out** what [decision 0252](0252-a-fixed-place-a-review-that-shows-what-runs-and-a-sign-in-before-its-probe.md) asked of `a2m-actions` and of these packs, with the rule of [decision 0248](0248-a-sign-in-goes-only-where-a-published-version-says-and-a-credential-belongs-to-its-target.md) that a sign-in's addresses come from the session keeper. The file pack adopted its part in [decision 0272](0272-the-file-packs-folder-writes-act-through-a-held-folder-after-a-second-reader.md). No action, schema, or catalog entry changes.

## Context

The compiler held a typed review summary of every version: for each tool what it starts and sets, every address it can reach by kind of place, and its fixed place; for each sign-in its provider, its credentials by name, and where it sends. Only the file pack filled it. The page a person publishes from printed text lines that the registry built by hand from a few of the summary's fields, and a second set of lines for the sign-ins. That page is the one moment a person sees what the tools they are about to give their agents will do.

## Decision

### 1. The person act carries the typed review

`PersonActAsk` gains `review: Option<ReviewSummary>`, re-exported at the root of `a2m-actions`. For `version.publish` it is the compiler's summary with each sign-in's addresses stated, in every environment, by `Keeper::sign_in_addresses` for the entry a published version would hold (`ReviewSummary::state_sent_to`). A provider the engine does not hold is stated as not held. Any other refusal of the keeper's (an address the target does not declare, a credential of another target, a setting the provider refuses) refuses the publish before the person is asked, as before. The keeper's function is now the one place a sign-in's addresses come from, for this page, the person's sign-in page, and the keeper's own checks.

Every text of the summary passes the redactor through the new `ReviewSummary::map_text`, which maps each address, path, argument, variable name and value, place name, file name, credential role, probe name, and description of what a sign-in sends where. The fixed sentences, the checked names, and the digests are left as they are.

The text lines built from `ReviewItem` and the sign-in lines are gone. `details` keeps the kernel's own sentences: the bindings that act for the first time, the implied `production` environment, and the gates.

### 2. The page renders it, sign-ins first

The act page renders the review in this order: the sign-ins (one whose probe is not drafted first, marked as not confirmed, then the file's order), with each environment's addresses grouped by kind of place, the riskiest first, and "may leave this machine" in bold; then the tools, in the summary's order, the most dangerous first, each with where it sends (a reach not fully stated first, in the fixed words), what it starts (a kept script with its fixed notice), its variables, and its fixed place; then where each surface leads; then the kernel's lines and the checks' warnings. The reason for the order is decision 0252's: a credential sent to an address puts every tool of its surfaces at risk. A variable marked secret is shown by name as "secret, not shown". Every text is shown with the characters that reorder or hide text written as their escapes, and escaped. An act with no review renders as before.

### 3. What each pack states

| Pack | Starts | Variables | Sends |
| --- | --- | --- | --- |
| `command` | Each program the binding's commands run (its own, its read-back's, its lookup's), by the surface's address and every digest its file pins; or the fixed pair: the interpreter and its digests, its fixed arguments, the script and its digest | Each `env.set` as set, with its value; each `env.pass` as passed | Not stated: what a program reaches is its own |
| `http` | Nothing | None | Only its surface: an endpoint's path is relative to the surface, the driver refuses any other address, follows no redirect, and uses no proxy |
| `mcp`, remote server | Nothing | None | Only its surface: the client follows no redirect and uses no proxy |
| `mcp`, local server | The surface's program, by path and digest | Each `environment` entry as set; each `pass_environment` name as passed | Not stated: the server is a program |
| `web` (`ui` and `page_http`) | Not stated | Not stated | Not stated |

No pack produces a secret variable today: the command pack refuses a credential in a `commands/` file and gives no credential to a program yet, and the MCP pack refuses a variable named for a credential. A value is plain text the target's file states.

**The web pack, checked.** A tab's own document goes only to the origins it was opened for, through every redirect. Its other requests do not: `Page::judge_request` in `a2m-pack-web-browser` refuses a request only where the tab observes and the method may write. An image, a script, a style, or a `fetch` sent with `GET`, `HEAD`, or `OPTIONS` passes to any origin, and in a tab that may act every request passes. So the surface and its declared destinations are not all a web tool can reach, and the pack says it does not state where it sends. Its lowerings answer `Statement::NotStated` on purpose, with that reason beside the code. What a tab runs is the scripts the system's pages bring, which the pack cannot state either.

## Alternatives weighed

- **The pack hands the page its own lines.** Refused for decision 0252's reason: a page would print sentences built from a target's words, and a pack that said nothing would not show.
- **Rendering the review in `a2m-actions` as HTML.** The registry would then hold markup for one front door. Refused: the registry hands typed values, and each face lays them out; the command line and an agent read the same act.
- **Redacting in the page.** The page has no redactor of its own. The registry redacts every text it hands a person, as it does for the input and the surfaces.
- **The web pack states its surface and declared destinations.** Refused: it would be untrue while a page's subresources pass to any origin. It becomes true when the browser base refuses every request outside the tab's origins and declared destinations, subresources included; then `UiLowering::statement` and `PageHttpLowering::statement` state `Sends::Only`.

## Not built

- The desktop, device, stream, and data packs state nothing: their tools say they do not state what they run.
- The web pack's statement waits on the browser base, as above.
- The place's own name in the plan and the lock (decision 0252).
- A secret variable: the review can show one (`VariableValue::Secret`), and no pack makes one yet.

## Consequences

- The page a person publishes from shows the program each command tool starts and its digest, and where each sign-in sends, before the tools.
- `PersonActAsk` has a new field; its one literal site is the registry's. No action, schema, catalog entry, or saved client changes.
- `a2m-local` depends on `a2m-files`, a lower layer, to read the summary's types.
