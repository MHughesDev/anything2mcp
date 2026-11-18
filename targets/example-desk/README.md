# The desktop test stand

Test-application case 28: one desktop program's accessibility tree, for the imaginary order-entry system named `example`. The desktop pack is proven against this stand and never against a real application. The pack's design is in [spec section 305](../../docs/spec/3-packs/305-the-desktop-pack.md) and [its crate design](../../docs/architecture/crates/a2m-pack-desktop.md); why the stand is built this way is [decision 0198](../../docs/decisions/0198-the-desktop-pack-reads-linux-applications-through-a-look-only-back-end.md); the rules every stand follows are in the [test application plan](../../docs/plan/test-application.md).

It is test equipment. A product crate may depend on it in its tests only.

## What it is

A library a test hosts in its own process. `Session::start()` starts a session bus of the test's own; the system's accessibility bus launcher and registry start behind it on demand. `Desk::start(&session, Order::example())` publishes one application, `example-desk`, on that accessibility bus over AT-SPI, as a toolkit does, and has the registry embed it. It draws nothing: what a desktop pack reads is the tree.

```text
application "example-desk"
  frame "Example order"
    panel
      label "Order number" -> entry "A-100"
      label "Customer"     -> entry "Example Supplies"
      label "Status"       -> entry "open"
      label "Tracking"     -> entry ""      (shows TRK-0001 when the control interface says so)
      entry with no name and no label
      label "Reference"    -> entry "R-1"
      label "Reference"    -> entry "R-2"
      label "Access code"  -> password text (holds a made-up secret, and answers it to anyone who asks)
      table "Line items": a row of column headers, then one row for each line
      push button "Save"   (a press is counted as a write)
```

## The control interface

`Desk::truth()` gives every method and property the application was asked, by name, with counts; how many times the password field's text was asked for; how many times Save was pressed; and what the `Tracking` field holds. `Truth::acted()` says whether any member that changes an application was asked. `Desk::show_late(after)` makes the `Tracking` field show its value once `after` has passed, and signals the change as a toolkit does.

## What it needs

Linux, `dbus-daemon` on the path, and the at-spi2-core programs (the file `/usr/share/dbus-1/services/org.a11y.Bus.service`). No display. The bus runs with a configuration of the stand's own, in a temporary folder, and every program it starts gets no display variable, a runtime folder of the stand's own, and settings kept in memory, so the test never touches a person's session. A machine without the programs, and every other system, gets `None` from `Session::start()` and its tests say they were skipped; with `A2M_REQUIRE_DESKTOP=1` that is a failure instead.
