# 0107. The `device` mode, and the device pack as building it settled it

**Status:** Accepted. Design decision. The owner had not read this record when it was merged.

**Date:** 2026-11-17

**Registers:** the mode `device`, of the device archetype, as item 8 of the bar in [spec section 202](../spec/2-kernel/202-kernel-and-packs.md) and [decision 0060](0060-a-mode-is-a-registered-name-and-the-archetype-is-the-closed-type.md) require when a pack is built. **Settles:** choices [spec section 310](../spec/3-packs/310-the-device-pack.md) left open or stated in a form the kernel does not take. The protocol and the range checks are [decision 0106](0106-the-device-pack-speaks-its-first-protocol-itself-and-checks-a-range-before-a-frame-is-built.md). The crate is `a2m-pack-device`.

## Context

Section 310 designs the pack in full, including writes that move something, firmware, and a safe-state operation. The package built the part that can be proven against [the simulated device](../../targets/example-device/README.md) without the engine loop: identification, reads, and writes with the hazard `none` or `device_function`. Its tests drive the driver through the kernel's handle and ask the stand what it stored. The crate's design document was not brought up to date by the package; it still says "Not started".

## Decision

**The mode.** `device`, registered by the provider `device`, with the verbs `read`, `update`, and `submit`; the step kinds `open`, `write_frame`, `read_frame`, and `expect`; the surface kind `device_link`, which states an `address`, a `protocol`, and a `unit` from 0 to 255; and the file kind `register`, in the folder `registers/`. The manifest declares one read and one write at a time on a session, and exclusive use of the link.

| Bar item | How it is met |
| --- | --- |
| 1. A driver with no retries inside | One frame a step, one reply read. Every step the lowering builds is declared not idempotent, so the engine repeats no write, even as `transient` |
| 2. A read-back rule for every write | `update` must read the written register again under `read_back`, and does not compile otherwise. A register with nothing to read, or whose reading changes it, can only be written by `submit`, which has no read-back and must be `irreversible` |
| 3. An answer for each identity strategy | `natural`, by the register's name in the map, and `none`. `composite`, `stamped`, and `find` are refused by the lowering with a reason |
| 4. A test stand with a control interface | `targets/example-device` |
| 5. A discoverer whose findings claim no more than was observed | `device_identity`: the device is asked to identify itself and nothing else, at `discovered` |
| 6. Failures mapped to the existing classes | The codes below. No class was added |
| 7. Effects and hazards declared | The manifest: the surfaces' network addresses, a session for `device_link`, changes a device. Every writable register declares a hazard |
| 8. A decision record | This one |

**How a verb lowers.** Every binding starts with `open`, which identifies the device against the binding's map once on a link.

| Verb | Act | Confirm | Read back |
| --- | --- | --- | --- |
| `read` | `open`; for each output, `write_frame` a read request, `read_frame`, `expect` the value | None | None |
| `update` | `open`; read the register's present value and keep it for reconcile; `write_frame` the write, the one step that commits | `read_frame`; `expect` an acknowledgement | `open`, which sends nothing on a link already identified; read each register named under `read_back`, which must include the written one |
| `submit` | `open`; `write_frame` the write, which commits | `read_frame`; `expect` an acknowledgement | None |

**What a write does, and how it is verified.**

- A write frame is sent only by a step that commits, under a posture that may write (live, or a sandbox with test records). The kernel's handle refuses a committing step without the ledger's intent token before the driver sees it.
- The value is checked against the register's range twice before the frame is built (decision 0106).
- The device's acknowledgement is evidence that it took the frame, never that it holds the value. The stand's device can echo a value and store another: a heater limit of 90 is echoed and stored as 80, and the read-back reports 80.
- A write frame that was sent and not clearly acknowledged is `possible_commit`/`no_confirmation`: the link closed, the reply was late or damaged, the reply acknowledges something else, or the device answered with a failure that does not say whether it applied the write. The link is then dropped, and nothing more is sent on it in that run.
- A device that answers a write with "busy" is `transient`/`device_busy`, a new code. It is not sent again by itself.
- A device that refuses with an address or value exception is `rejected`/`device_refused`.

**Identity of the device.** `open` reads the device's identification and stops with `unknown_variant`/`device_identity_mismatch`, for reads and writes alike, when any field the map lists under `identify` is not reported, when the model differs, or when the firmware version is not in the map's `firmware` list. A step that names a register of a map the device was not compared with sends nothing. The map must list `model` and `firmware_version` under `identify`.

**Pace.** The gap between frames is the larger of the map's `min_gap` and what its `max_rate` allows. A reply must come within the map's `reply_within`, and never after the run's deadline. A map with no pace takes the pack's settings: 1000 ms for a reply, 50 ms between frames.

**Evidence.** A step keeps the frame it sent, as hexadecimal, and the length of the reply. The reply's content leaves the driver only as values, each passed through the redactor.

**Discovery and drafting.** The first protocol offers no listing of a device's registers, so identification is all there is to observe, and the discoverer reads no register and scans no address. The drafting rule turns an identity finding into the start of a map: its model and firmware version, and no register, range, hazard, safe state, or unit.

## Not built

Each is refused by name, or is declared data nothing acts on.

- **Writes with the hazard `physical`.** The compiler refuses them with `device.physical_not_built`. `confirm_by`, `safe_state`, and `on_link_loss` compile into the plan and nothing acts on them. The safe-state operation, the alert that offers it, and the kill switch reaching the link are not built.
- **Firmware.** Neither inspection nor update. `device_not_returned`, `device_protected`, and `image_protected` are not raised, and the manifest does not declare them.
- **One frame that writes several registers.** A binding writes one register (`device.one_register_per_write`).
- **Reading a register whose reading changes it** (`device.read_has_effect`), and **writing text** (`device.text_not_writable`).
- **Serial links, and a console on a link.**
- **Reconcile against the safe state** for a device that reverts on link loss. The value before a write is kept for reconcile; the comparison with the safe state is not written.
- **The engine loop.** Section 310's build order step 2 asks for five runs with one trace signature through the engine loop. The test computes that signature over five runs through the handle; the engine has not run the mode. The exclusive lock on the link, and `busy`/`session_in_use`, are the scheduler's and are not exercised here.
- **The tests have run on Linux only.**

## Where the code differs from section 310

| Section 310 says | Built | Why |
| --- | --- | --- |
| `types.yaml` gains `scale`; `pump_speed_rpm: {base: measurement, unit: rpm, scale: "0.1"}` | `scale` is on the register in the map, beside `data`. The field type is `{base: decimal, places: 1}` | The kernel's measurement units are a closed list of lengths and weights, with no speed, rate, or temperature. The scale is the wire encoding, which section 310 itself says belongs to the map. This settles the open point on where `scale` lives |
| A readable, writable register with the hazard `none` is effect `write` | The example operations that write such a register are `irreversible`, and still read it back | A register is found by its name in the map, and the kernel takes a record key only from an input, so these operations declare no identity |
| The kernel checks the range before the preview | The driver checks it twice | Decision 0106 |
| The device is identified by identifier, model, and firmware | Model and firmware are compared. Every field listed under `identify` must be reported; the serial is read and not compared | A map holds a model and a list of firmware versions, and nothing to compare a serial with |
| The default pace is one frame at a time and "a few per second" | One frame at a time, 50 ms between frames by default, which allows twenty a second | A setting a person can lower. Whether it is conservative enough is open |
| The drafter writes a map of readable registers and one `read` operation for each | A map with the model and the firmware version and no register | The first protocol lists no registers, and a published map file is not read yet |
| A choice in `safe_state.holds` is written by its name: `inlet_valve: closed` | By its stored code: `inlet_valve: "0"` | A choice register is held as the text of its code |
| The failure table | Adds `transient`/`device_busy` and `input`/`payload_unreadable`. `session_in_use`, `approval_missing`, and `read_back_differs` are not the pack's: the scheduler, the policy engine, and the engine's comparison raise them. `sensor_out_of_tolerance` is not raised | As above |
| The mode is proposed | It is registered by this record, for what is built | Item 8 of the bar |

## What the provider contract and the compiler could not express

No kernel edit was made. Each is done by the pack, or left out, as stated.

| # | Section 310 needs | The kernel has | The pack |
| --- | --- | --- | --- |
| 1 | Identify the device before acting, as a precondition | No precondition a pack can declare | Every binding starts with `open`, and the driver refuses any register step on a map the device was not compared with. Section 310 already proposes a kernel precondition |
| 2 | Identity `natural` by the register's name | A record key taken only from an input | Writes are declared `irreversible`; the read-back still runs and is reported |
| 3 | A safe range on a value | Field types with no range | The range is in the map; the driver checks it |
| 4 | A unit such as rpm | A closed list of measurement units | Exact numbers with places |
| 5 | `confirm_by`: a sensor within a tolerance after a settle time, reported as its own fact | One settle time before read-back, and a read-back compared for equality after normalization | Not built |
| 6 | A run's budget that counts frames | A step's time limit and a run's deadline | Each reply is bounded by `reply_within` and the deadline; frames are not counted |
| 7 | A stopped run offering the safe-state operation in one action; the kill switch reaching the link | Nothing a pack registers for either | Not built |

## Consequences

- Section 310 still calls the mode proposed, still puts `scale` in field types, still says the kernel checks the range, and its failure table lacks `device_busy`. The lead owes those edits, and the crate's design document owes its build order and interface.
- Writes that move something, the reason the pack exists, are still to be built. Until they are, a target cannot bind a `physical` register at all, which is the safe side.

## What would change it

- A kernel precondition for identity, a record key a pack may name, or a range on field types: rows 1 to 3 would move into the kernel, and writes to a hazard-`none` register could become `write`.
- Building `physical` writes will show whether `confirm_by` fits the contract's read-back, or needs a second kind of fact; that is the next test of hypotheses H-26 and H-34.
