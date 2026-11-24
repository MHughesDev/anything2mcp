# 0999. The macOS stand shows each value field as a text field, whether or not it is editable

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-24

**Changes:** the macOS form of the desktop test stand, `targets/example-desk/src/mac/window.rs`, which [decision 0399](0399-the-desktop-pack-writes-on-windows-and-macos-through-a-closed-list-of-its-own-on-each.md) made. The macOS back end of the desktop pack, its role table ([decision 0244](0244-the-desktop-pack-reads-macos-applications-and-proves-the-record-before-every-act.md)), and the shared screen model are unchanged.

## Context

After [decision 0409](0409-a-macos-window-keeps-its-key-for-as-long-as-it-is-listed.md), the writes of `tests/write.rs` reached Save on macOS, and four of them still failed there alone: an update and the resume after a silent Save stopped as `possible_commit`/`no_confirmation` at the step's deadline, a refused value stopped the same way instead of as `rejected`/`app_validation_error`, and two writes at once ended `busy`/`session_in_use` behind the first one's hold. The press itself is answered at once (the stand's own test counts the write as soon as `AXPress` returns), so the window did show its answer, and the driver could not find it.

The one screen model the three stands share names the confirmation as `{role: entry, name: "Message"}` and the refusal as `{visible: {role: entry, name: "Problem"}}`. On Linux those are read-only entries, and on Windows read-only edits, which UI Automation gives as edits. On macOS they were `NSTextField`s made not editable, and AppKit's accessibility says a text field that is not editable is a static text (`AXStaticText`), which the role table maps to `label`. A label named by its title element is never read for its value, so the confirmation's condition found no entry `Message`, which reads as not holding, and the refusal's found no entry `Problem`: the wait went on to the deadline. The same answer would have made the read-only fault, which makes `New status` not editable, a field that is not found instead of one that is read-only.

## Decision

1. **Each value field of the macOS stand says its role is a text field** (`AXTextField`), editable or not, by answering the role itself as it already answers its value and its title element. Whether a client may set its value is answered as before: not while the field is not editable or not enabled. The stand so shows what the Linux and Windows stands show, a read-only entry, and the shared screen model finds it on all three.
2. **The back end's role table stays the system's.** `AXStaticText` is still a `label`, named by its title, its title element, or the text it shows.

*Instead of* mapping a static text that names a title element to an `entry` in the back end: that is a guess about what the application meant, made in the one table that should say only what the system says. *Instead of* a macOS screen model of its own: the stands exist to prove one model on three systems, and a second model would prove two halves.

## Consequences

The stand's own test on macOS requires `New customer`, `New status`, and `Message` to be text fields that name their labels, and the back end's walk of the stand requires them as entries and finds no entry `Problem` while it is hidden. The proof is CI's macOS job; Linux and Windows are unchanged.

A real macOS application that shows a value in a text field it made not editable shows a static text. A screen model for it names a `label`; when that static text names a title element, its name is its label's and its value is not read, so a confirmation shown there cannot be told by its text. That is not built: a target that needs it is a reason to read a static text's value as text, with a record.
