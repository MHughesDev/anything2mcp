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
      push button "Save"   (a press is counted as a write; it stores the edit fields' values when the application takes them)
      label "New customer" -> editable entry (holds at most 24 characters, and cuts the rest)
      label "New status"   -> editable entry
      label "Message"      -> entry "Saved." once Save stored
      label "Problem"      -> entry, shown only while the application refuses a value
```

What is typed into an edit field is not stored until Save, and the labelled values `Customer` and `Status` show what is stored, so they differ from the edit fields until then. Save stores only a customer that is not empty and a status of `open`, `held`, or `closed`; otherwise it shows why under `Problem` and stores nothing ([decision 0218](../../docs/decisions/0218-the-desktop-pack-writes-on-linux-through-a-second-closed-list-and-reads-back-what-is-stored.md)).

## The control interface

`Desk::truth()` gives every method and property the application was asked, by name, with counts; how many times the password field's text was asked for; how many times Save was pressed; and what the `Tracking` field holds. `Truth::acted()` says whether any member that changes an application was asked. `Desk::show_late(after)` makes the `Tracking` field show its value once `after` has passed, and signals the change as a toolkit does. For a write, `Truth` also reports what is stored, how many times Save stored, and every value set into each field with its window; `Desk::fault` switches on, for the next press of Save, a Save that stores nothing and says nothing, a Save that stores and then leaves the bus before it confirms, or a confirmation that arrives late, and, until the stand ends, a read-only field; `Desk::open_window(title)` opens a second window with a `New customer` field of its own; `Desk::restart()` starts the application again with what it stored. `Fault::AnotherRecordAfterRead(number)` makes the window show another order number once its order number has next been read, and `Fault::DialogAfterRead` makes the application raise a dialog titled `Example notice` at the same moment: the two ways an application changes under a reader between its look and its act ([decision 0244](../../docs/decisions/0244-the-desktop-pack-reads-macos-applications-and-proves-the-record-before-every-act.md)). The write half is built for Linux only; on Windows and macOS these return an error.

## On Windows

The same window, made of plain Win32 controls, in a process of its own ([decision 0209](../../docs/decisions/0209-the-desktop-pack-reads-windows-applications-through-ui-automation-on-a-thread-of-its-own.md)). `Session::start()` checks that the process's window station is the interactive one, takes the one seat of the test process, and copies the test's own program into a folder of its own as `example-desk.exe`; `Desk::start` starts that copy with the one test `example_desk::stand_child!()` declares, which every test binary that starts a stand must declare. The window is shown on the screen without taking the focus. Each control is subclassed and counts, by control and name, the messages another process sends it that read or change it, every message that asks the password edit for its text, and Save's notice to its window as a write. It cannot witness what a UI Automation client answers without a message to the control. The control interface is a line protocol on the process's standard input and output.

## On macOS

The same window, made of AppKit's own controls, in a process of its own, through the same harness as on Windows ([decision 0244](../../docs/decisions/0244-the-desktop-pack-reads-macos-applications-and-proves-the-record-before-every-act.md)). `Session::start()` first asks the system whether this test process may use the accessibility interface (without its prompt): a client that may not reads nothing, so without the permission the session is `None` and the tests say they were skipped, and with `A2M_REQUIRE_DESKTOP=1` it fails and says the permission did not take for this program. It then takes the one seat of the test process and copies the test's own program into a folder of its own as `example-desk`. AppKit makes windows on a process's main thread only, which no test runs on, so `example_desk::stand_child!()` also declares a function the system calls as it loads the program, before the test harness begins; in the stand's process it serves the window and the control interface there and never returns, and in every other process it returns at once. The window is shown without making the stand the active application.

The text fields, the secure text field, and Save are subclasses of AppKit's own that count each time AppKit's accessibility asks one for its value, sets its value or its focus, or presses it, and Save counts each run of its action as a write. The stand cannot witness a question AppKit answers without asking the control itself (a title, a role, the children, a label's text, a table's cells), nor what a client does with an answer; whether AppKit asks the subclass's own methods for every request is unproven until a run on macOS shows it, and the stand's own test checks it for the password field.

## What it needs

Linux, `dbus-daemon` on the path, and the at-spi2-core programs (the file `/usr/share/dbus-1/services/org.a11y.Bus.service`). No display. The bus runs with a configuration of the stand's own, in a temporary folder, and every program it starts gets no display variable, a runtime folder of the stand's own, and settings kept in memory, so the test never touches a person's session. On Linux, a machine without the programs gets `None` from `Session::start()` and its tests say they were skipped; with `A2M_REQUIRE_DESKTOP=1` that is a failure instead. On macOS the stand needs a window server session and the Accessibility permission for the test program (above).
