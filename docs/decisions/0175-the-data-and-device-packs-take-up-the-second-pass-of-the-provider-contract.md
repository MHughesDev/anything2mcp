# 0175. The data and device packs take up the second pass of the provider contract

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Changes:** the designs of [`a2m-pack-data`](../architecture/crates/a2m-pack-data.md) and [`a2m-pack-device`](../architecture/crates/a2m-pack-device.md). Builds on decisions [0100](0100-the-mode-sql-and-the-data-packs-contract-as-building-it-settled-it.md), [0137](0137-a-device-is-held-to-its-serial-a-write-frame-is-recorded-before-it-is-sent-and-a-run-has-a-frame-ceiling.md), [0161](0161-the-plan-and-the-engine-after-the-packs-were-built.md), and [0162](0162-the-provider-contracts-second-pass-what-a-step-is-handed-and-hands-back.md).

## Context

The kernel's second pass added to the contract only what a pack takes up by choice. Two packs took up what applies to them. Four choices were real.

## Decision

| Point | Built | Instead of, and why |
| --- | --- | --- |
| The key a store assigns, on a resume | The read-back of a write whose statement returns an assigned key names no argument for the key's parameters. The driver binds each from `Prepared::recorded()`, only for a key parameter of a read, and stops the step unsent (`input`/`value_not_bindable`) when it is neither given nor recorded. The lowering allows the gap only in the read-back of such a write | `record.key` in an expression: its parts are text, and a key column of another type would be bound as text. `StepInput::key`: the engine hands it only once a key is known, and the recorded value is the typed one the store returned |
| A database file below the request's project | For a surface whose root is `project`, the driver resolves the address below `CheckedPaths::project_folder()` when the run was handed no path under the surface's name. A run with no project folder stops by name before anything is opened. The pack's own roots remain for any other root a manifest declares | The pack's root `project`, which is the engine's own folder: two projects served by one engine would share it |
| What the store said of a refused constraint | `said` holds the constraint's name, table, and column for a server, and the part after `constraint failed:` for an embedded store's unique, not-null, and check constraints. Never the server's message or detail, which quote a row. The driver redacts it before it leaves `act` | Keeping the whole message: it can quote a value |
| A device read set beside the recorded frame | A read of a register for which `frame_<register>` is recorded reads the frame back into the write it carries (`Link::written`) and reports whether the register holds what that write would have put there, as the capture `frame_check` and in `measured`. The read's value is unchanged and the verdict stays the engine's | Failing the read when the two differ: a device that rounds a value would stop every verified write, and a lost write would stop its own reconcile |

One fix came with it: a commit sent into a link that had already ended is now `not_sent` and not `answered`, as the server tests always expected; the test that held it was racing.

## The two writes with no read-back, described (decided below)

- **Device `submit`.** It writes a command and reads nothing back: the step after it only checks the echo. It is `irreversible` by `least_effect`, so a person approves every run. It is the one kind of write in the device pack with no read-back. It could be `WriteEvidence::Acknowledged`; that was not done, since the brief for this pack lists none.
- **Data `create` with identity `none` and no `read_back:`.** The lowering accepts a `form: table` write that names no read-back, and the compiler accepts it when the operation is `irreversible` with identity `none`. The test fixture for an assigned-key insert had none until this record; it now names one. Whether the pack should require one is open.

## Consequences

- The five-run proof for a read is a test of each pack, through the handle.
- The device pack's read-back path is proven with no key: the record's `key` is `None` and no step reads it.
- Nothing here changes a plan's shape, except that the data fixture's `example_shipment_create` carries a read-back.

## The two writes with no read-back, as the lead decided them

- **Device `submit`** states `WriteEvidence::Acknowledged`: the device's echo is the only evidence there can be for a command that sets no readable value. `Lowering::evidence` returns it for a `submit` whose lowered read-back is empty, and `read_back` for everything else; an `update` keeps its read-back by address. The operation is `irreversible` by `least_effect`, and the fixture `example_fault_reset` declares it and compiles with evidence `acknowledged`. The frame is still recorded before it is sent (decision 0137), so a lost echo leaves a record in doubt with the frame in what was recorded.
- **Data, a write that names no `read_back`.** The pack states `Acknowledged` only for an operation declared `irreversible` with identity `none`, where the store's count of exactly one row, checked inside the transaction before the commit, is the evidence. Every other `form: table` write that names no `read_back` is refused by the pack's own rule, `data.read_back_invalid`, with a fix that says to name the read the pack writes by key; the pack no longer rests on the kernel's rule alone. The assigned-key insert fixture keeps the read-back it was given.
