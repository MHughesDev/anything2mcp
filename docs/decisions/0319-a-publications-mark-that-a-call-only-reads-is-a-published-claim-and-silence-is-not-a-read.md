# 0319. A publication's mark that a call only reads is a published claim, and silence is not a read

**Status:** Accepted. Design decision, made under [decision 0294](0294-the-owner-hands-the-open-questions-to-the-agents.md). The owner had not read this record when it was merged. Supersedes in part point 1 of [decision 0276](0276-what-the-scout-may-learn-from-a-program-itself-and-by-which-means.md); its other points stand.

**Date:** 2026-11-23

**Changes:** what the readers of decision 0276 point 1 record; [SCOPE.md](../../SCOPE.md); hypothesis H-41. Section 305 is to follow (see the end).

## Context

Decision 0276 point 1 builds readers of what a program publishes about itself "where the publication says which calls only read". [Research note 33](../research/33-what-a-program-can-reach-in-another.md) read what each publication says. A COM type library marks a member as a property get, a property put, or a plain function, and a plain function carries no mark about its effect ([read]). A macOS scripting dictionary marks a property read-only or read-write, and Apple's own guide says Cocoa does not enforce the mark ([read, as summarised]). D-Bus introspection gives a property an access value and says nothing of a method ([search, memory]). No publication says of every call whether it only reads, and one that does say may not be held to it by its own system.

## Decision

1. A reader of a publication records, for each call, one of three: **the publication marks it as a read** (a property get; a property marked read-only or read; a property read through the standard properties interface); **it marks it as a change**; or **it says nothing**.
2. A call the publication says nothing about is never treated as a read. It is drafted, if at all, as a write that a person must declare otherwise.
3. A mark the publication makes is the author's claim. It is recorded at the level of a published claim, never above it, whether or not the system enforces it; where the system is known not to enforce it (the scripting dictionary's read-only mark), the finding says so.
4. A reader still never starts the program it reads: a type library is read as a file, a dictionary from the application's bundle, and D-Bus introspection only of a name that is owned now (decision 0276 point 1, unchanged).
5. For a scripting route, when [decision 0214](0214-ways-through-the-refusals-that-keep-a-person-from-their-own-machine.md) row 6 is built, two points of note 33 are carried as **open points to verify on each system before that row is accepted**, because the note has them from search summaries and memory only: that macOS's Automation grant belongs to the sender the system judges (the interpreter, or the program that started it), so one grant covers every script it runs; and that a message addressed to an application by name starts it when it is closed. If both hold, row 6's declaration names its target by bundle identifier and says whether the target must already be running, and the review summary says in the person's words what the grant covers.

## Alternatives

| Alternative | Why not |
| --- | --- |
| Keep point 1 as written, trusting a publication's read marks | The formats cannot carry it for methods, and one system does not enforce its marks |
| Treat a property get as verified read | The library declares a shape; a get can still have an effect |
| Make point 5 rules now | Their facts were not read from a primary source |

## Proof

When the first reader is built: a test publication with one marked read, one marked change, and one unmarked call drafts the first as a read at the published-claim level and the other two as writes; H-41's run against the test stand compares the stand's own record of what was stored for one marked read and one marked change.

## Owed to files this record does not edit

Section 305 (the desktop pack) is owed, from note 33: a refused row for starting an application by a message addressed by name; a short part on what the person must grant on each system; and a read of an element's enabled and read-only state before every set on Windows, with the macOS settable check marked to verify against Apple's page when the write slice is built. The desktop pack's design is owed a reserved failure code for a missing Automation grant, if a scripting route is built.
