# 0456. The first release leaves out what its first hour never meets, and a shortcut is offered only when every step is built

**Status:** Accepted. Design decision, made in the design review round by lens 7, less ([report](../reviews/less-review-2026-11-25.md), findings 1 to 6). The owner had not read this record when it was written. It moves one package and half of another after the first release, adds package LS1, and changes the first-hour path that section 404 shows. It changes no owner decision and no rule.

**Date:** 2026-11-25

**Changes:** the [roadmap](../plan/roadmap.md) (FT5 split, FT5b added and held; LS1 added) and [the line](../plan/build-order.md) in the Build Order; [section 404](../spec/4-building-tools/404-setup-and-ease-of-use.md) (the first-hour commands; starter templates and `init --embed`); [section 220](../spec/2-kernel/220-actions-and-the-authoring-interface.md) and the [action catalog](../reference/action-catalog.md) (the shortcuts, once LS1 is built); refines, of [decision 0397](0397-what-the-first-release-needs-of-the-packages-left-and-what-waits-until-after-it.md), the list of actions left unbuilt.

## Context

The first release is close. Lens 7 asks what can be removed from it without losing the core. The core is what [H-18's result](../hypotheses/H-18-result.md) measured six times: an agent with no knowledge of the repository runs `init`, `add`, `ready`, its person presses once, and the agent runs `tool search` and `tool call`. In the second measurement each of three agents did that in 15 to 20 commands and under a minute and a half. None of them used a setting, a shortcut other than `init`, `add`, and `ready`, or any of the 27 actions that have no handler.

What the round found against that path:

- **Four shortcuts do nothing.** `diff`, `bundle`, `rollback`, and `migrate` each run one action that has no handler, so each answers "does nothing yet". They are shown in the command tree beside `ready`, which works.
- **One shortcut undoes itself.** `learn` is `capture.start`, `capture.stop`, `endpoints.infer`. `capture.start` asks for a capture that only its person begins, on the engine's page; `capture.stop`, run straight after by the same shortcut, ends a capture that has not begun, which is `EndedBy::NotBegun` in `crates/a2m-actions/src/handlers/captures.rs`, with nothing recorded. The shortcut then stops before `endpoints.infer`, which has no handler. `crates/a2m-actions/src/tests/second.rs` asserts the two steps and the stop, and does not look at what the capture became.
- **Package YL1 adds, and protects nothing that fails open.** It names a breaking change to the person who publishes ([decision 0439](0439-a-tools-name-carries-no-version-and-a-breaking-change-is-named-at-publish.md), points 2 and 3). Without it, an agent's next call to a tool whose input became required stops as `input`/`schema_invalid`, named, before anything is sent. The page already lists every tool, most dangerous first. Nothing YL1 adds is frozen by the first release's kept contract ([decision 0437](0437-what-a-release-publishes-is-kept-and-every-later-build-is-checked-against-it.md)), because `version.diff` has no handler and so is not kept; it can come in any later release as an addition.
- **Package FT5 waits on two things for one kind.** Its last recipe, a desktop application, waits for package 8.3 and for a person's machine. The desktop pack already writes on Linux, and its Linux CI job runs against an accessibility bus with no window drawn; a desktop application on Linux in the field-test box needs neither 8.3 nor a person. Windows and macOS do.
- **Section 404 shows a path nobody takes.** It gives `init`, `add`, `learn`, `test`, `publish`, `connect`, `serve`, and promises starter templates and `init --embed`. None of the last two is built, and the measured path needs neither: the scout drafts from the system itself.
- **The list of unbuilt actions in decision 0397 has 23 names; there are 27.** The four it lacks are `endpoints.infer`, `run.undo`, `screen.snapshot`, and `unknown.settle`. The guidance file names `screen.snapshot` as an action that "looks at one thing", and it answers `action_not_built`.

## Decision

1. **A shortcut is offered only when every one of its steps is built.** The command tree, `--help`, and the catalog's table of shortcuts hold only such shortcuts, and a test over `SHORTCUTS` holds it. `diff`, `bundle`, `rollback`, and `migrate` leave the first release; each comes back with its action, in the same change that gives the action its handler. The behaviour of a shortcut that stops before an unbuilt step, and its two notes, go with them, since no shortcut can then reach one.
2. **`learn` is `capture.start` alone.** It asks for a capture of the person's own use and gives the page's link. The person begins and ends it on that page; `capture stop` afterwards reads how it ended and scouts what it found ([decision 0451](0451-a-capture-is-scouted-when-capture-stop-finds-it-ended-and-a-scout-that-saw-nothing-does-not-answer-ok.md)). `endpoints.infer` stays without a handler, after the release, for running the inference again over stored findings, as decision 0400 keeps it.
3. **YL1 stays before the first release, built.** The round proposed moving it after the release, because nothing of it is frozen by the release and its absence fails closed. YL1 was built and merged (decision 0454) while the round ran, so it is done and this point moves nothing. Decision 0439 stands as it was.
4. **FT5 is split.** FT5 keeps the desktop application on Linux, in the field-test box, before the release. It needs the box's pinned image ([decision 0447](0447-the-field-test-box-takes-a-pinned-image-a-user-that-is-not-root-and-a-named-filter-for-the-browsers-sandbox.md)) to hold a session bus, an accessibility bus, and one application that publishes its tree, which the machine of field test 9 could not fetch ("The kinds not run in FT5" in [the field test results](../reviews/field-tests.md)); the image is made where it can be fetched and pinned by digest. FT5b, the same recipe on Windows and macOS, is after the release: it needs a person's machine, and package 8.3's writes there.
5. **Section 404 shows the measured path:** `init`, `add`, `ready`, the person's press, then `connect` for a client. Starter templates and `init --embed` are not in the first release: nothing in the first hour needs them, and each would be a second way to start a project that the first-hour test does not hold.
6. **The actions with no handler (27 when the round counted, 26 once YL1 was built) stay in the catalog, unbuilt, after the release.** They are the 23 of decision 0397, and four more; `version.diff`, one of the 23, now has its handler (YL1), so 26 are left: `endpoints.infer` (point 2); `run.undo`, which builds a reverse write that a person approves, Phase 9; `screen.snapshot`, a look at one screen, for which the scout and `draft.try` stand on one machine; and `unknown.settle`, since a held draft's question is settled today by writing the draft with `draft.write`. Each answers `action_not_built`, is not listed up front, and has no client method. Package 5.1's test holds this list. `engine.start`, `engine.stop`, `engine.logs`, and `self.update` are carried by the client and are not among them.
7. **Kept before the release, each for a reason this lens weighed:** N0b, because the names it renames in results and help are frozen by the kept contract at the release, so renaming them now costs nothing and later costs a listed break; FT6, the proof of Stage 3 and the product's main case; 8.3, which a worker holds; 5.1; 5.6c, which also makes the guidance give the short way first; and YL2.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Remove the unbuilt actions from the catalog for the first release | Each is a designed interface with a record; they cost a user nothing they can reach by accident, since none is listed up front or has a client method, and removing them would make the catalog and section 220 disagree with the designs that name them |
| Keep `diff`, `bundle`, `rollback`, and `migrate`, each answering "does nothing yet" | A person who reads the help finds four of eighteen words that do nothing, and an agent that reads it may plan around them |
| Remove `learn` as well | The capture is built and is the product's main case for a web application with no description; one word for it earns its place once it does the right thing |
| Keep YL1 before the release (what happened: it was built first) | It is three crates of work whose absence fails closed, and nothing about it is frozen by the release |
| Cut the desktop pack, or the device pack, from the first release | Each is built and proven against its stand; cutting either removes software from a person's reach, which is the North Star's first measure, to save nothing a user pays for |

## What it costs a user

A person who republishes a target is not told that a tool's input became required; their agent's next call is stopped by name, and the person sees why in the run's error. A person on Windows or macOS uses a desktop pack that has met no existing application. A person who wants to compare, bundle, roll back, or migrate a version has no command for it, as before, and is no longer shown one.

## Proof

Package LS1's: the test that every shortcut's steps are built; `learn` from the command face answers with the capture's page and leaves it waiting for its person, and the capture is not ended; the command tree's help has no `diff`, `bundle`, `rollback`, or `migrate`; section 220 and the action catalog list the same shortcuts as `SHORTCUTS`.

## What would reopen it

A measurement or a field test in which a republish broke an agent and nobody could tell why (YL1); a person who could not undo a publish without a `rollback` command; or a first-time user stopped for want of a starter template.
