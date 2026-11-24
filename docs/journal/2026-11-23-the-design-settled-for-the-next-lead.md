# 2026-11-23: the design settled for the next lead

The entry before this is [the first field test](2026-11-23-the-first-field-test.md), written early in the same day. This one covers the rest of the day, and it is written for a lead that takes over with no memory of it.

## State at the end

- **On `main`:** pull request 20: the field test wiring, the reader's fixes, the fault proxy, the build plan, and decisions 0293 to 0300.
- **In pull request 21, open:** the holding folder ([decision 0298](../decisions/0298-a-draft-with-an-open-question-waits-in-a-holding-folder.md)), the proper-name rule ([decision 0301](../decisions/0301-every-part-has-a-proper-name.md)), the architecture page's source, the build of [decision 0302](../decisions/0302-a-field-named-as-a-secret-is-never-drafted-as-an-input-or-an-output.md), and decisions 0303 to 0309. It merges when every check is green.
- **Progress:** 52 of 146 packages, 35% (`python3 tools/progress.py`).
- **Stopped:** no system of a field test was left running.

## What the owner decided

- Every part a document or a drawing shows has a proper name (decision 0301).
- The agent that leads is the sole architect: it may leave any plan for a better one, and writes down why. Inferred from the owner's words; the ten rules and the owner's records still bind.
- The lead changes to a smaller model after this entry. The owner asked that the design be finished first, so that what is left is building. That is why decisions 0302 to 0309 were written in one sitting.

## Done

- The holding folder, built twice: the first design, in which the compiler set files aside, failed a review and was dropped.
- FT8: a field named as a secret is no drafted input or output. Field test 1 then compiled the system's whole description, three runs alike.
- Eight decision records, 0302 to 0309, and in [the build plan](../plan/build-order.md) a section, "The design is settled: build in this order", with the steps of each package.
- The architecture page, built from data in `docs/architecture/page/`.

## Measured

- Field test 1: 152 reads drafted in place and 69 files held, before decision 0302. After it the whole target lints clean.

## Learned

- A test that needs a real server does not run in a plain workspace test. One such test of the data pack failed in CI after the holding folder and was found only there.
- The trial action is not in the build toolset's list: it is the thirteenth marked action and the list holds twelve, three of them not built. [Decision 0306](../decisions/0306-the-short-path-is-add-then-a-trial-and-only-a-built-action-is-listed-up-front.md) follows from it.
- A field test cannot approve a write or sign in, because the program rightly ships no presence prompt a program can pass. [Decision 0304](../decisions/0304-a-field-build-stands-in-for-the-person-in-a-field-test.md) follows from it.
- The audit's own tests fail on a machine where workers' copies lie inside the repository folder, and pass in a clean copy.

## Noticed, not acted on

- Decisions 0303 to 0309 were not argued against by a second model. The field build (0304) and the signed fixture (0307, point 2) stand next to the approval rule and deserve it before they are built.
- The compiler's test for a secret's name takes `secretary` for one and does not take `accessToken` for one. The drafter checks the output's name too, so nothing is drafted wrong; the rule itself is unchanged.
- 287 names coined for the architecture page are in `docs/architecture/page/proposed-names.md`, unreviewed.

## Decided without the owner having read it

Decisions 0294 to 0300 and 0302 to 0309. Each says so in its status line.

## Next

The first row not struck through in the build plan's table: S2, then FT7, V4 and V1, V3, GD1, FT4, FT9.
