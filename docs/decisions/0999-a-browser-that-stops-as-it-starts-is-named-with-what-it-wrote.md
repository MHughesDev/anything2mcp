# 0999. A browser that stops as it starts is named with what it wrote

**Status:** Accepted. Design decision, made by the worker on field test 9's findings 54 and 55 under the lead's brief. The owner had not read this record when it was written. The web pack's browser engine gains one failure code, `busy`/`browser_stopped_at_start`, and one launch reason, `LaunchReason::Stopped`; its fixed launch arguments gain three flags.

**Date:** 2026-11-25

**Changes:** `a2m-pack-web-browser` (`src/launch.rs`, `src/browsers.rs`, `src/error.rs`, its failure code table, a test program in `examples/`); `a2m-pack-web-ui`'s `codes::PASSED_ON` and manifest; the designs of [the browser engine](../architecture/crates/a2m-pack-web-browser.md) and [the web interface pack](../architecture/crates/a2m-pack-web-ui.md); the [field test results](../reviews/field-tests.md), findings 54 and 55.

## Context

Field test 9 ran the engine's browser in a container ([decision 0447](0447-the-field-test-box-takes-a-pinned-image-a-user-that-is-not-root-and-a-named-filter-for-the-browsers-sandbox.md)). Where the browser could not make its sandbox, run as root or under the container engine's default system-call filter, it exited before it answered over its pipe, and the engine reported it as `unknown_variant`/`browser_not_owned`, "not a browser the engine can own", with the next step "Check the browser program named in the settings, and its version". The program was a browser, and the person was sent to the wrong thing (finding 54).

The browser says why on its standard error. The supervisor, in the kernel's provider contract, sends a foreign link's standard error to the null device, so the pack never sees it. Measured on this machine, with the stable Chrome of the field test's box image, run with `--enable-logging --log-file=<file> --log-level=2`:

- as root: exit 1, and the file holds `ERROR: ... Running as root without --no-sandbox is not supported. See https://crbug.com/638180.`;
- as a user that is not root, under the default filter: the process aborts, and the file holds `FATAL: ... Zygote process exited prematurely with exit code 82 (setuid sandbox: failed to move to new namespaces)`.

## Decision

1. **The browser writes its errors to a file in its own data directory.** The fixed launch arguments gain `--enable-logging` and `--log-level=2`, and each launch names `--log-file=<data directory>/launch-errors.log`. Level 2 is errors and worse. A page's console is logged at a lower level, and not at all from a context that keeps its state in memory, which every tab is in. The data directory is the engine's and private, and it already holds the session's cookies, so the file holds nothing more sensitive than the folder around it.
2. **The file is this start's alone.** It is removed before each start. Once the browser has answered, it is removed again; where the system keeps an open file from being removed, as Windows does, the next start removes it.
3. **A pipe that ends before the first answer, after the program wrote why, is `LaunchReason::Stopped`**, `busy`/`browser_stopped_at_start`, which does not pass by itself. Its message is the browser's own: its fatal lines first, then its errors, at most two, each without the bracket the browser puts before it, with every known secret replaced and then cut to 200 characters. Its next step says to act on what the browser wrote, and where it names its sandbox, to run the engine as a user that is not root and let that user make user namespaces, in a container with a filter that allows them. The engine never starts a browser without its sandbox (rule 5).
4. **Everything else stays `browser_not_owned`**: a pipe that ended with nothing in the file, something that is not the protocol, another protocol version, an unknown digest. A silent start with its pipe open stays `transient`/`timeout` ([decision 0134](0134-one-browser-starts-at-a-time-and-a-silent-start-is-slow-not-foreign.md)).

## Alternatives

- **The supervisor keeps a bounded tail of a foreign link's standard error.** It shows what any program wrote, not only a browser, and needs no file. It changes the provider contract's supervisor, a kernel crate outside this work's crates, and the `ForeignLink` every pack receives. Not done now; it remains the better place if a second pack needs a program's own words.
- **`--enable-logging=stderr`.** The supervisor sends standard error to the null device, so it would reach nobody.
- **A new code under `unknown_variant`.** What stopped the browser is a fact of the machine that a person must change, as with `browser_unavailable`, so the class is `busy`, and it does not pass by itself.

## Consequences

- A person whose browser cannot sandbox itself is told so in the browser's own words. Field test 9's two variants, run again with this change, are named `browser_stopped_at_start` with "Running as root without --no-sandbox is not supported" and with "Zygote process exited prematurely with exit code 82 (setuid sandbox: failed to move to new namespaces)". The scout's answer shows the code under "not looked at"; the message is in the run's own record (finding 56 is the scout's).
- The test `a_browser_that_stops_as_it_starts_is_named_with_what_it_wrote` starts the crate's test program `examples/stops_as_it_starts.rs`, which writes the line such a browser writes and a planted secret, and exits; the failure names the line, holds no secret, and a second start reads only what it wrote. `cargo test` builds the program.
- Finding 55 was not this change: on staging, which holds [decision 0445](0445-the-first-hours-faults-a-list-on-the-command-line-a-key-off-a-list-and-a-project-held-to-one-store.md), field test 9's recipe drafts `screens/orders_all_orders.yaml`, and the scout proves all three reads.
