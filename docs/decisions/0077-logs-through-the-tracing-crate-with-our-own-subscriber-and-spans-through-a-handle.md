# 0077. Logs through the `tracing` crate with our own subscriber, and spans through a handle

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Settles:** the outside crate for logging in the [`a2m-telemetry` design](../architecture/crates/a2m-telemetry.md), and the form of its span and metric interface. **Changes that interface from the design's sketch.**

## Context

Every crate needs to log, and the engine needs spans and metrics that tests and the benchmark harness can read. Section 217 says logs are one JSON object per line, levelled, rotated by size, and redacted; that no span or metric may carry a value; and that nothing leaves the machine unless switched on.

## Decision

**Logging**

- **Crates log with the macros of the `tracing` crate.** Version 0.1.44, MIT, with only its `std` feature. It was already in the workspace's lock file, brought by the HTTP crates the test stands use, so it adds no new crate: `tracing`, `tracing-core`, `once_cell`, and `pin-project-lite`, all MIT or Apache-2.0. It builds on Windows, macOS, and Linux. The licence check passes.
- **The subscriber is ours,** about a hundred lines. It keeps events, formats each as one JSON object, redacts every text, checks the finished line with `contains_secret`, and writes it. A line that still holds a secret is replaced by a fixed line that says one was dropped. *Instead of:* `tracing-subscriber` and `tracing-appender`, which bring a registry, a formatter, and a writer thread we would then have to wrap for redaction and rotation by size, and more outside crates with them.
- **Rotation is ours too:** by size, a numbered file per rotation, the oldest deleted. The file is closed before it is renamed, which Windows requires.
- The attribute macros are off, so no procedural-macro crates are built for logging.
- A span made with the `tracing` macros is accepted and ignored. Its fields are free text, and a kept span may not carry any.
- `start` installs the subscriber for the process. A second `start` in one process takes the logs over; the last one started receives them. A telemetry made in memory receives log lines only inside `Telemetry::scoped`.
- Writing is synchronous, under a lock, one write per line. *Instead of:* a writer thread. If the benchmarks show logging on an async thread costs too much, a thread is added behind the same interface.
- *Instead of the `log` crate:* it is smaller still, and the HTTP crates in the tree already speak `tracing`, so one framework reaches every line.

**Spans and metrics**

- **They are methods on a `Telemetry` handle,** which the engine already receives in its dependencies. *Instead of:* the free functions in the design's sketch, which need state shared by the whole process and cannot be read back by a test without it.
- A span's parent is given, as a `TraceContext`. There is no current span kept per thread: the engine's work moves between threads.
- A span's name is a `&'static str` from the source, or the MCP method and tool. Its attributes come from the closed `Attr` type. A metric's name is a `&'static str` too. So neither can carry text made at run time.
- Time comes from a `Clock` given at the start. `ManualClock` moves only when told, so a test gets exact durations.
- Finished spans wait in a buffer of 4,096; counters and histograms of durations are kept per series. Histograms use fixed bounds in microseconds and whole numbers only.
- A trace id that starts here is made from the time of day and a counter. It is unique within the process and is not random. The slice that builds export replaces it with a random source.
- **Export to an outside collector is not built.** There is no exporter and no cargo feature for one yet, and a test holds the crate's outside dependencies to four that open no connection.

## Consequences

- Crates above write `tracing::info!` and the like, and take `tracing` from the workspace with the same features.
- The engine's design names `telemetry: Telemetry` in its dependencies, which is this handle.
- `start` does not fail when the log folder cannot be written. It logs to standard error and reports `busy`, `storage_full` through `Telemetry::log_destination`.

## Alternatives considered

Given beside each choice above.

## Revisit if

Logging shows up in the engine's benchmarks, or the OpenTelemetry exporter needs span data this collector does not keep.

## Spec sections

- [217-traces-evidence-and-repair.md](../spec/2-kernel/217-traces-evidence-and-repair.md)
- [The `a2m-telemetry` design](../architecture/crates/a2m-telemetry.md)
