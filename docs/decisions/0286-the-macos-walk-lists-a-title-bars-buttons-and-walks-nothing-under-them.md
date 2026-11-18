# 0286. The macOS walk lists a title bar's buttons and walks nothing under them

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-18

**Changes:** the walk of [decision 0244](0244-the-desktop-pack-reads-macos-applications-and-proves-the-record-before-every-act.md), point 6, in `crates/a2m-pack-desktop/src/backend/macos.rs`. The rest of that record stands.

## Context

On the hosted macOS runner, for pull request 16, the test `a_snapshot_is_the_windows_shape_and_changes_nothing` in `crates/a2m-pack-desktop/tests/draft.rs` discovered the stand's window twice and got two different findings. The window had not changed. The only difference was inside the second of the three unnamed buttons the system draws in the window's title bar: the first walk found a group inside a group there, the second found one group. The same test passed on the same runner in pull request 14 ([decision 0278](0278-what-the-first-runs-on-windows-and-macos-showed-about-the-desktop-pack-and-the-check-of-programs.md)), so what the system puts inside that button changes from one walk to the next. A snapshot that changes while the window does not would show drift in every application on macOS, since every titled window has these buttons.

## Decision

The macOS back end lists each button of a window's title bar (the subroles `AXCloseButton`, `AXMinimizeButton`, `AXZoomButton`, and `AXFullScreenButton`) as a `push_button`, and walks nothing under it. None of what the system draws there is the application's, no locator names it, and the pack never acts on it.

The back end's own test against the stand now requires that the walk lists at least one such button and that none has an element under it.

## Alternatives weighed

- Compare two snapshots leaving out what is under any button. That weakens the test's claim, that the same window gives the same finding, and leaves the finding itself unstable for every caller.
- Leave the title bar's buttons out of the walk altogether. They are part of the window a person sees, and a snapshot that lists them costs nothing; only what is inside them is unstable.
- Stop walking under every `push_button`. Wider than the evidence: an application's own button may hold an element a screen needs.

## Consequences

Unproven until the macOS job runs it: which of the four subroles the hosted image gives each button, and that nothing else in a titled window's frame changes between walks. If the job shows another element that changes on its own, it is named the same way.
