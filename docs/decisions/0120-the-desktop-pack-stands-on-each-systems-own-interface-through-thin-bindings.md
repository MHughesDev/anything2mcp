# 0120. The desktop pack stands on each system's own interface, through thin bindings

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the first open point of the [desktop pack's design](../architecture/crates/a2m-pack-desktop.md), the library or libraries; research brief 19; roadmap package 8.1. Changes the design's registration table, its failure codes, and its build order.

## Context

The desktop pack finds a control by role and name, reads it, acts on it through the interface's own patterns, and ends every wait on an event. It must do so through UI Automation on Windows, the Accessibility interface on macOS, and AT-SPI on Linux. [Research note 19](../research/19-desktop-accessibility-base.md) ran the steps on Linux against two test windows, type-checked them for Windows and macOS, and compared one library over the three systems, a wrapper crate per system, and bindings of our own.

## Decision

1. **Linux: `atspi-proxies`, `atspi-common`, and `atspi-connection` over `zbus`, on Tokio.** `atspi-connection` without its default `p2p` feature: with it, opening the connection failed while an application was still starting. Safe Rust, no C, no build-time system library, typed events from the bus. Proven on Linux.
2. **Windows: Microsoft's `windows` crate, with the UI Automation features only, under a small safe layer of our own.** *Instead of:* `uiautomation`, which is safe at its surface but does not compile without its synthetic-input feature, waits in its matcher by sleeping in a loop, and brings a date crate.
3. **macOS: `objc2-application-services` and `objc2-core-foundation`, under a small safe layer of our own,** with the observer's run loop on a thread the back end owns. *Instead of:* the `accessibility` crates, which stand on deprecated bindings.
4. **No library over all three systems.** *Instead of:* `xa11y`, whose waits poll every 100 ms, which carries synthetic input and screen capture in the same crates, compiles Objective-C on macOS, is synchronous, and changes fast. One interface over three systems would also hide two differences the run showed a driver must see: how a field's name is computed, and which event confirms a change.
5. **`unsafe` is confined to one module per system,** on Windows and macOS, with `#[allow(unsafe_code)]` on each function and a comment on each block, as `a2m-provider` and `a2m-local` already do for their system calls. Each such function is exercised by a test on its system.
6. **A wait is woken by an event and decided by a fresh read.** A condition is evaluated by reading the element again when any event concerning it arrives, never from the event's payload. Finding the application and its window are waits like any other.
7. **A field's name is the platform's accessible name:** its own name, else the name of what labels it (the labelled-by relation on Linux, LabeledBy on Windows, AXTitleUIElement on macOS). Each back end states its rule, and a test holds it on its system.
8. **Two failure codes,** both class `busy`: `accessibility_off` (the macOS permission is not granted, or the Linux session's switch is off) and `target_elevated` (a Windows window at a higher integrity level). The pack states the fix and never works around either: it shows the macOS prompt only on a person's command, and never flips the Linux switch, which is the person's setting and outlives the session.
9. **Test-application case 28 is one Rust program in egui,** without its bundled fonts, whose licences are off the closed list. A second window of native controls on each system joins it when that system's back end is built.
10. **Build order: Linux, then Windows, then macOS.** Linux is the only system on which the steps have run, and CI can run it under a virtual display. Windows hosted CI machines are reported to give a desktop. macOS hosted machines are not expected to grant the permission, so its back end is proven on a machine where a person grants it.

## What was shown

On Linux, seven runs against a GTK 3 window and an egui window: the field found by role and name, set, its event in 0 to 5 ms, read back, Save invoked, its confirmation by event, and exactly one write in the window's store each time. A wait no event ends failed at its deadline. On Windows and macOS: the same steps type-check, with 16 `unsafe` blocks each. Every candidate's tree passes the licence check with the closed list unchanged.

## Alternatives considered

- **`xa11y`.** Above.
- **`uiautomation` on Windows.** Above. It would remove the Windows `unsafe` layer, which is its real merit.
- **Our own D-Bus proxies on Linux.** They would drop the second set of runtime crates that `atspi-proxies` turns on in `zbus`; not worth it while the proxies are thin and maintained.
- **Polling where an event is missing.** Refused by the design: such a condition is a lint error for that system.

## Revisit if

- A Windows or macOS run shows a call or event behaving otherwise than its binding says.
- `uiautomation` builds without its input feature and offers an event-driven find; then it may replace the Windows layer.
- `atspi-proxies` stops forcing `zbus`'s own runtime, or stops being maintained.
- A walk of a large window proves too slow and the AT-SPI cache interface is needed.
- The macOS permission can be given on a hosted CI machine, or cannot be given on any machine CI can use.

## Spec sections

- [305. The desktop pack](../spec/3-packs/305-the-desktop-pack.md)
- [106. What it latches onto](../spec/1-product/106-what-it-latches-onto.md)
