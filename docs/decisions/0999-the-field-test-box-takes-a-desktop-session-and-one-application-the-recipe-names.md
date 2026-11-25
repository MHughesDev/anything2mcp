# 0999. The field test's box takes a desktop session, and one application the recipe names

**Status:** Accepted. Design decision, made by the worker on FT5's desktop application under the lead's brief. The owner had not read this record when it was written. No public interface of the product changes: the change is to the field-test runner, which is test equipment.

**Date:** 2026-11-25

**Changes:** `tools/field_test.py` and its tests; a new file `tools/field-box/desktop-session.sh`; the [field harness's design](../architecture/crates/a2m-field-harness.md); roadmap FT5 and its rows in the [Build Order](../plan/build-order.md); the [field test results](../reviews/field-tests.md), field test 10.

## Context

FT5's last part before the first release is a field test of a desktop application on Linux, in the field-test box ([decision 0456](0456-the-first-release-leaves-out-what-its-first-hour-never-meets-and-a-shortcut-is-offered-only-when-built.md), point 4). The desktop pack's Linux back end reads an application through AT-SPI: it asks the session bus named by `DBUS_SESSION_BUS_ADDRESS` for the accessibility bus's address, asks the registry on that bus which applications it lists, and proves the one the surface names by the process the bus daemon gives for its connection, the program that process runs, its user, its start time, and its digest ([decision 0198](0198-the-desktop-pack-reads-linux-applications-through-a-look-only-back-end.md)). It starts nothing. So the box must already hold, when the engine asks: a display for the application's windows, a session bus, the accessibility bus with its registry, and the application, all run by the user the engine runs as.

The box held none of these. Decision 0447 let a recipe name the box's image by digest, its user, and a named filter, which is what a browser needed; a desktop application needs processes started in the box before the engine asks, which no option of the runner started. Field test 9's entry "The kinds not run in FT5" said that no image the project could reach held an application with an accessibility tree. On this machine, on 2026-11-25, a base image pinned by digest could be fetched through a registry mirror, and its operating system's package source answered inside a container, so an image could be made.

Measured while this was built: the session bus daemon does not start for a user that has no entry in the image's password database ("Could not get password database information for UID of current process"); the accessibility bus launcher keeps its socket under `XDG_RUNTIME_DIR`, or under `$HOME/.cache` without one, which in the box is the engine's home; a GTK 3 application in the box published its tree to the registry with no accessibility switch turned on and no other part of a desktop.

## Decision

1. **A recipe may ask for a desktop session in the box: `"box_desktop": {"application": ["/absolute/program", "argument", ...]}`.** It holds that one key. The application is an argument list whose first part is the program's absolute path, with no `..`; the recipe names the program and its arguments, so the repository names no application (rule 9). A recipe whose system is that application alone says `"system": {"kind": "desktop"}`, names no image, and starts no system's container; `exec`, `http`, and a proxy are refused in it, as for a folder.
2. **The session is a script the repository keeps, `tools/field-box/desktop-session.sh`, mounted read-only in the box.** With the box, at the first `a2m` or `desktop` step and before the field harness starts, the runner runs it detached in the box, as the box's user (`start`). It starts a virtual display that listens on no TCP port, a session bus whose socket is in a folder of the box's own (`/tmp/a2m-desktop`, mode 0700, which is also the session's `XDG_RUNTIME_DIR`, so nothing lands in the engine's home), and the accessibility bus launcher; it waits for each, by its socket or by the session bus's answer, within a bound; then the application takes the script's place. The script refuses to run as root, and stops by name when the image lacks one of its programs.
3. **The runner waits for the application on the bus, and stops by name otherwise.** It asks, at most `DESKTOP_WAIT` (60) seconds, through the same script (`ready`), whether the registry lists an application whose process runs the program; the run otherwise stops as `desktop_not_ready`, with the last lines of the session's logs. A step `"desktop": {}` lists every application the registry holds, by its connection, program, and name, asked of the bus by the runner and never through the product, so a recipe can show what the box held before and after the product read it.
4. **The harness and each command of the product get the session's `DISPLAY` and `DBUS_SESSION_BUS_ADDRESS`, and nothing else of it,** as a person's shell has them. The engine finds the session bus where a person's engine does, from its environment.
5. **What the runner refuses.**
   - A `box_desktop` without a `box_image`: the runner's own Debian image holds no display and no bus.
   - A `box_desktop` without a `box_user`, or with root: a desktop session is a person's.
   - A `box_desktop` with any key but `application`, or an application that is not as point 1 says.
   - Every refusal of decision 0447 holds unchanged: the image by digest only, its entry point never run, the filter by name only and never unconfined, and a recipe holding a flag that switches a browser's sandbox off, in the application's arguments as anywhere in its text.
   - Nothing of this machine's display or buses reaches the box: the session adds one read-only file to the box's argument list and nothing else, no device, socket, capability, or security option. A test holds the argument list to that.
6. **The image is the recipe's, made where it can be fetched, and pinned by the digest the container engine gives it.** A recipe kept outside the repository holds the file the image is built from, on a base image pinned by digest, and the list of the packages and versions it holds; the image must also hold `Xvfb`, `dbus-daemon`, `dbus-send`, `/usr/libexec/at-spi-bus-launcher`, the application, and a password entry for the box's user. The engine's own store names a local build by its manifest's digest, and the runner runs it by that digest as any other; it is not pushed to any registry. Another machine that builds the same file gets other package versions and another digest, so the recipe's list is what a second run is compared with.

The report states the session's application and the script's hash, beside the box's image, user, and filter.

## Alternatives

| Alternative | Why not |
| --- | --- |
| A step that starts any program in the box in the background | A recipe could then start anything in the box beside the engine, including a second engine; the session starts the three parts a desktop needs and one application, and nothing else |
| The image's own entry point starts the session | Decision 0447 never runs an image's entry point, so that the box runs only what the runner starts; an entry point is also the recipe's code, which nobody in the repository reviews |
| This machine's own display and session bus, mounted into the box | It hands the box this machine's desktop and every application on it; a field test reads only what the run started |
| A published image that holds a display, the buses, and an application | None the project could reach held all three with an application that is not built on a browser runtime, which the desktop pack does not read (spec section 305); and a published image of another's choosing would still need the session started |
| Turn the session's accessibility switch on before the application starts | The switch is the person's, and the pack never changes it; GTK 3 published with it untouched, so the runner leaves it alone too. A toolkit that publishes only with it on is a later recipe's question |
| The image pushed to a registry run for the test, to get a registry's digest | The engine's own store already names the build by a digest that pins it; a registry adds a container and a step and pins nothing more |

## Proof

`tools/test_field_test.py`, the class `DesktopSession`: without the option the box has no session and its commands no display or bus; an application is an argument list from an absolute program, and every other shape is refused; a session needs an image of its own and a user that is not root; the session adds one read-only file to the box's arguments and nothing wider; the harness and each command get the session's display and bus and nothing else; the session starts once, detached, and waits for the application on the bus; an application that never reaches the bus stops the run as `desktop_not_ready` with the session's own words; a `desktop` step needs the session; a desktop recipe is held to the refusals of decision 0447; the session's script names no application and listens on no TCP port; it refuses a call it does not know, and parses.

The field test of a desktop application on Linux, recorded as field test 10 in the [field test results](../reviews/field-tests.md), runs the box with the session, three times, from `init` to a published read that is called.
