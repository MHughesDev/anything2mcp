# 0409. A macOS window keeps its key for as long as it is listed

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** the key of a window in the macOS back end of the desktop pack, which [decision 0244](0244-the-desktop-pack-reads-macos-applications-and-proves-the-record-before-every-act.md), point 1, gave as `window:<listing>.<index>` with the last four listings kept. Supersedes that part of point 1; the rest of decision 0244, and [decision 0399](0399-the-desktop-pack-writes-on-windows-and-macos-through-a-closed-list-of-its-own-on-each.md), stand.

## Context

The first macOS run of the desktop pack's writes (decision 0399) failed four tests of `tests/write.rs` with `transient`/`connection_lost`, nothing sent: "the element is from a read this back end no longer holds". A write's `goto` keeps the window it found as a key of one listing. Every `set` and `invoke` after it walks that window, and its `before_act` lists the application's windows again, to prove that no unnamed window is open. Each listing took a number from the counter walks also take, and kept only the last four listings, so after a set or two the `goto`'s listing was let go, and the next walk of its window stopped. On Windows a window's key is its handle, made afresh from the system on every use, so the same write passed there; on Linux it is the element's own path on the bus.

## Decision

1. **A window's key is a number of its own, kept while the window is listed.** The macOS back end keeps each window a listing found, with its process, under `window:<number>`. A later listing gives a window it already holds the same number: the two are the same element as the system compares elements, which asks no application. A window that a listing of its application no longer shows is let go, and its key is then a stop as before. Listings take no number from the walks' counter, so they no longer push walks out either.
2. **Nothing the proof holds is loosened.** Every use of a key proves the application's process again first, the key must be of that process, and every act still proves the process, the record, and the absence of an unnamed window, on the walk it acts on, through the closed list.

*Instead of* keeping more listings: any fixed count is a count of steps a write may take before its window is lost, and a longer write would meet the same stop. *Instead of* finding the window again by its role and title on each walk: that is the `goto`'s judgment repeated in the back end, and two windows with one title would make it a guess.

## Consequences

The stand's own test on macOS lists its window eight times more and requires the same key each time. The fault is in the macOS back end alone, so its proof is CI's macOS job; Linux and Windows are unchanged.
