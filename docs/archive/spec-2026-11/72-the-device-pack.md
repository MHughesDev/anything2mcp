# 72. The device pack

> Archived on 2026-11-17. This is section 72 of the first design, kept as it was written, with the names and numbers of its time. What it says now lives in [section 310, The device pack](../../spec/3-packs/310-the-device-pack.md).

Devices reached through their own interfaces, with their firmware. This is the device archetype of [section 54](54-interaction-archetypes.md): exchanging frames with hardware over a bus or a link. [Section 63](63-devices-and-firmware.md) gives the rules and [record 0054](../../decisions/0054-devices-through-their-own-interfaces-and-firmware-through-the-devices-own-update.md) fixes them. This section is the pack's design under those rules, and it loosens none of them. It answers each item of the entry bar in [section 55](../../spec/2-kernel/202-kernel-and-packs.md). Proposed mode: `device`, for a fieldbus or a framed link. A mode becomes real only by decision record when its pack is built.

A device is the place where a write can move something in the world. Everything in this pack is arranged around that. Where the engine loop fits a device well, this section says so; where it strains, it says so too.

## What it is for, and what it refuses

A device is reached in one of three ways, and only one of them is this pack's.

| The device offers | Served by | What this pack adds |
| --- | --- | --- |
| A management protocol over a network | The request packs: HTTP ([section 62](62-catalog-of-surface-kinds.md)), or a later mode of the request archetype | Nothing in code. The rules of section 63 apply through kernel vocabulary: hazards, approvals, serialized use, a declared safe state. One gap is named under "Identity" below |
| A console reached by starting a program, such as a remote session client | The command pack ([section 68](../../spec/3-packs/306-the-command-pack.md)) | As above |
| Registers or objects on a fieldbus; a console on a serial line or a raw socket with no program between | This pack, mode `device` | Everything below |

A console on a link is a framed link whose frames are lines of text: one declared command out, one answer in, parsed by a strict pattern. Section 54 places it here, and so does this section.

| Refused | Why |
| --- | --- |
| A debug probe, for reading or writing | Not designed; the worst outcome is permanent ([section 63](63-devices-and-firmware.md)) |
| An exploit, an unlock, or a way into a device it does not offer | Rule five |
| Defeating secure start-up, a signature check, or read-out protection | Rule five. A protection that refuses is a named stop |
| Writing memory, or any address not in a declared map | Not an interface the device offers for the purpose |
| Scanning addresses to see what answers | A read is not always without effect on a device (below) |
| A free-form console line from a caller | Section 61: commands are not a shell |

## The file kind: `registers/`

One file per device model and map. It holds every address the pack may touch, how each is encoded, whether it may be written, its safe range, and its hazard. Units and scale live in field types, as rule eight requires; an operation names a register, never an address. A device with an object dictionary instead of a register table uses the same file kind, with `index` and `subindex` as the address.

```yaml
# types.yaml (kernel file), the field types the map uses
pump_speed_rpm: {base: measurement, unit: rpm, scale: "0.1", places: 1}
valve_state:    {base: choice, codes: {0: closed, 1: open}}
```

```yaml
device_map: controller.main
surface: controller                  # a surface of kind device_link in target.yaml
model: example-controller
firmware: ["2.4.0", "2.4.1"]         # versions this map was proven against; others stop
identify:
  reads: [vendor, model, firmware_version, serial]
pace: {min_gap: 20ms, reply_within: 500ms, max_rate: 20/s}
registers:
  pump_speed_measured:
    address: {table: input, offset: 30}
    data: u16
    type: pump_speed_rpm
    access: read
  pump_speed_setpoint:
    address: {table: holding, offset: 100}
    data: u16
    type: pump_speed_rpm
    access: read_write
    range: {min: "0", max: "1450"}
    hazard: physical
    confirm_by: {register: pump_speed_measured, within: "25", settle: 5s}
  inlet_valve:
    address: {table: coil, offset: 4}
    data: bit
    type: valve_state
    access: read_write
    hazard: physical
  fault_latch:
    address: {table: input, offset: 7}
    data: u16
    access: read
    read_effect: clears              # reading it resets it; never used for read-back
safe_state:
  operation: example_controller_pump_stop
  holds: {pump_speed_setpoint: "0", inlet_valve: closed}
on_link_loss: {device_reverts_to_safe_state: true, after: 2s}
```

| Field | Meaning |
| --- | --- |
| `address`, `data` | Where a value is and how its bits are laid out: width, signedness, word order. The wire encoding; it belongs to the map |
| `type` | A field type: unit, scale from the raw value, places. A raw value is turned into a decimal by its scale; a float register is decoded exactly and rounded to the declared places before any comparison |
| `access` | `read`, `write`, or `read_write`. A `write` register has nothing to read: a command such as "start" or "reset" |
| `range` | Required on every writable register that holds a number. The safe range in the field type's unit |
| `hazard` | Required on every writable register. `none` is allowed only where the developer declares it |
| `confirm_by` | A sensor register that shows the world followed, with a tolerance and a settle time |
| `read_effect` | `none` by default; `clears` or `advances` for registers whose reading changes them |
| `safe_state`, `on_link_loss` | What safe means for this device, the operation that reaches it, and what the device itself does when the link drops |

The surface kind `device_link` names the link: a network address and port, or a serial port with its settings; the protocol; and the unit address on a shared bus. A binding of mode `device` names a `device_map`, a `verb`, and `registers`, a map of register to expression.

## Verbs and lowering

The steps are those of section 54: `open`, `write_frame`, `read_frame`, `expect`.

| Verb | Effect class | Lowers to |
| --- | --- | --- |
| `read` | `read` | `open` with identify, then `write_frame` a read request, `read_frame`, `expect` the reply's form |
| `update` | `write` or `irreversible`, by the table under "Hazards" | Identify; read the current value; check the range; `write_frame`; `read_frame` the acknowledgement; read the register again |
| `submit` | `irreversible` | Identify; `write_frame` a command to a `write` register or a declared console command; `read_frame` the acknowledgement; `confirm_by` where declared |

A sequence of device writes is a pipeline of these operations, one commit point each. A single frame that writes several registers at once is one commit point, read back register by register.

## The driver, part by part

| Part | What the driver does |
| --- | --- |
| Prepare | Opens the link, or takes it from the supervisor, with **exclusive use** for this run. Reads the device's identity and compares it with the map: model and firmware version |
| Act | Sends one frame or one declared console command, built from the map and typed values. Nothing else is sent in that step |
| Confirm | The device's acknowledgement: an echo of what was written, or a reply of the declared form, within `reply_within` |
| Read back | Reads the register again. A register with a `read_effect` is never read back; a write to it is `irreversible` |

**The link sets the pace.** A link carries one request at a time; the driver waits for each reply, keeps `min_gap` between frames, and never exceeds `max_rate`. Where the map gives no pace, a conservative default applies: one frame at a time and a few per second ([section 63](63-devices-and-firmware.md), rule 7). The step's time limit is the map's `reply_within`; the run's budget counts frames, not only time.

**A lost link after a write** means the frame may have been applied. It is `possible_commit`. On reconnection, reconcile identifies the device first, then reads the register. If the map declares that the device reverts on link loss, the value read is compared with the safe state as well as with the intended and the earlier value, and a reverted value is reported as such, not as a mismatch. A `submit` has nothing to read, so it stops for a person.

## Hazards are the centre of this pack

| Register | Effect class | Hazard | Least approval |
| --- | --- | --- | --- |
| Readable, writable, hazard `none` declared by the developer | `write` | `none` | As policy says |
| Readable, writable, settings that keep the device working | `irreversible` | `device_function` | A person, every run; no standing approval; no pre-approval |
| Readable, writable, moving or heating or opening something | `irreversible` | `physical` | A person, every run; no standing or pre-approval; a second person where policy asks |
| A command register or console command | `irreversible` | As declared; `none` is refused for these | As the hazard says |

An operation with a hazard other than `none` is `irreversible`, as section 61 says. A read-back still runs and is reported as its own fact, matched or not, but it does not make the write resumable.

**Every writable register carries a hazard,** and the compiler refuses one without. The drafter never writes one.

**Range is enforced before a frame is sent,** twice. The kernel validates the input against the field type and the range and stops with `value_out_of_range` before the preview. The driver checks the encoded raw value against the same range, converted by the field type's scale, before it builds the frame, so no rounding or overflow can carry a value outside it.

**There is no automatic retry** of any device write, whatever its class. The pack declares its writes not idempotent to the engine, so `transient` never repeats them. A device reached over noise or a shared bus cannot prove that a frame with no reply did nothing.

**A safe state when a run stops halfway.** A pipeline that opens a valve and then starts a pump may stop after the first. The engine does not continue it, undo it, or send anything further: a stop means the device or the link is in a state with no rule, and more frames into that state is a guess. Instead the stop is recorded with what landed and what may have, a person is alerted, and the alert offers the declared safe-state operation as one action. The person invoking it is the approval it needs. The kill switch reaches the link and runs the same operation. The device's own fail-safe, declared as `on_link_loss`, is the first line; the safe-state operation is the second.

A `physical` operation must declare `confirm_by` where the device has a sensor for it. The setpoint read back proves the device took the value; the sensor within its tolerance after its settle time proves the world followed. They are two facts, reported separately.

## Identity

**A record is a register or an object,** found by its name in the map. Identity `natural`, always. A command has no record and is identity `none`.

**A device is identified by what it reports:** identifier, model, firmware version. That is the subject of every finding and every piece of evidence ([section 58](../../spec/2-kernel/218-discovery-and-findings.md)). Prepare compares it with the map before any frame that is not identification. A device that does not match is `unknown_variant`, reads included, because a register's meaning may change with firmware. This is stricter than [section 59](../../spec/2-kernel/219-the-evidence-ladder.md), where a current version keeps running until a canary fails; for a device, section 63 wins.

A new firmware version drops the bindings on that surface to `reconstructed` until the gates pass again with the new version added to the map's list.

Devices reached through the request and command packs need the same identity check before acting. It has no kernel home today; it is proposed as a precondition the kernel offers to every pack.

## Sessions and secrets

| Rule | How |
| --- | --- |
| One link, one session | The open link is the session's live state, held on the home runner by the supervisor ([section 57](../../spec/2-kernel/204-supervising-subsystems.md)) |
| No sharing between runs | One run holds the link at a time, through a lock the scheduler grants. A second run waits or gets `session_in_use`. Each run identifies the device again |
| Credentials and keys | In the vault, injected when the link opens. Never in a frame the trace keeps; frames are stored with secrets removed by structure |
| Links with no sign-in | Many fieldbus protocols have none. Then consent carries the weight: the person names the device ([section 61](../../spec/2-kernel/213-effects-hazards-and-consent.md)), and the surface is declared and nothing else is reachable |

**Effects** in the manifest: uses the network or a serial port, for the declared links; holds a session; changes a device, for the declared links. All `enforced`.

## Firmware

**Inspection is discovery only.** An optional provider, off by default, reads an image the person supplies as a file, or one the device offers through its own interface for that purpose. Its subject is the image by digest; its findings are `inferred`; it lists regions, files, and settings, and decrypts nothing. The image is never changed. An image that is encrypted or protected is a named stop: class `edge_blocked`, code `image_protected`.

**An update goes only through the device's own documented mechanism.** Where the fieldbus protocol defines one, this pack carries it; where the device's management protocol does, the request pack does, under the same rules.

| Stage | This pack's part |
| --- | --- |
| Before publishing | The operation must declare its recovery route, what cannot be undone (counters that only rise), the preconditions the device's documentation sets, and whether the device checks the vendor's signature. The compiler refuses a firmware operation without a recovery route |
| Precondition | Identify; current version; each declared precondition read from the device |
| The image | A file input, stored as a blob, its digest recorded. The target may list the digests it accepts; one not listed stops with `input` |
| Approval | A person, every run, shown current and proposed version, digest, signature checking, and what cannot be undone |
| Act and confirm | The mechanism, and nothing else. The device accepts the image and checks it |
| Read back | After a declared restart window: identify again, and compare the version and digest the device reports |
| If it does not come back | `possible_commit` with the code `device_not_returned`; a person is alerted with the recovery route. Nothing is retried |

An update is always `irreversible`, with the hazard `device_function`. The pack never builds, patches, or signs an image, and never checks a signature in the device's place.

## Discovery and drafting

| It reads | Finding | Level |
| --- | --- | --- |
| What the device answers when asked to identify itself | The device subject: identifier, model, firmware | `discovered` |
| A listing the device computes of its objects, where the protocol offers one | Objects, with access and data type | `discovered` |
| A published map of registers or objects, as a file | Registers, with addresses, types, and units where stated | `inferred` |
| A firmware image, through the optional provider | Regions and settings | `inferred` |

The discoverer reads only addresses the device lists or a published map names, and never one marked or described as changing on read. The scout tries the identification read, and reads of objects the device itself lists as readable. On a register table that lists nothing, it tries only identification; the rest waits for the developer's `draft.try`.

The drafter writes a map of readable registers with their data types, and one `read` operation for each. It writes no write, no range, no hazard, no safe state, and no unit the device or its map did not state.

## Where the loop fits, and where it strains

| Part | Fit |
| --- | --- |
| Prepare, act, confirm, read back | Fit cleanly: one frame, one acknowledgement, one register read again |
| Read is without effect | Not always. A clear-on-read register breaks the assumption; the map must say so, and such a register is never read back |
| Read-back proves the outcome | Only that the device holds the value. The world is a second, declared fact |
| Resume | A device write is never resumed automatically. Reconcile reads, and a person decides |
| Retry by class | Overridden: no device write repeats, even as `transient` |
| A stopped run | Leaves something in the world. The design answers with a declared safe state a person can reach in one action, not with an automatic undo |

None of these needs a kernel change beyond the identity precondition above. They are evidence for hypotheses H-26 and H-29.

## Failures

| Class | Codes | When |
| --- | --- | --- |
| `input` | `value_out_of_range`, `schema_invalid` | Outside the declared range or type; nothing is sent |
| `policy` | `surface_not_declared`, `approval_missing` | A link not declared; a hazard without its approval |
| `busy` | `session_in_use` | Another run holds the link |
| `unknown_variant` | `device_identity_mismatch`, `unexpected_reply` | Model or firmware not in the map; a reply of no declared form |
| `rejected` | `device_refused` | The device answers with an exception: an address or a value it will not take |
| `edge_blocked` | `device_protected`, `image_protected` | A protection on the device or the image refuses |
| `transient` | `timeout`, `connection_lost`, `frame_corrupt` | On a read, or before a write frame was sent |
| `possible_commit` | `no_confirmation`, `device_not_returned` | A write frame was sent and no clear acknowledgement came; an update did not come back |
| `mismatch` | `read_back_differs`, `sensor_out_of_tolerance` | The register or the declared sensor disagrees |

Codes not already in the technical reference are proposed.

## The test stand

A simulated device in software, in the test application family, speaking the chosen protocol on a local port. No real hardware runs in CI. It holds registers, a small simulated process (a pump whose measured speed follows its setpoint with a lag), and an update mechanism with two slots.

| Control call | Reports or does |
| --- | --- |
| Reset, Inspect | Restore known state; return every register and the state of each simulated output |
| Frames | Every frame received, decoded, in order, with how each was answered |
| Count writes | How many times each register was written |
| Faults | Apply a write and drop the reply; delay; corrupt a frame; refuse with an exception; report another firmware version; revert on link loss; restart and stay down; refuse an image |

Against the simulator, read-back is exact and ground truth is complete. That shows the pack is right. It does not show a real device behaves the same. Before support for any protocol is claimed, one real device model is tried as a test environment with settings that are safe to write, at the end, with the owner, and with the owner's go-ahead in that session (rule ten).

## What it needs from outside

Stated as needs, each an outside crate to be verified: a client for the chosen fieldbus protocol, asynchronous on Tokio, under a licence on the closed list, on all three operating systems; for serial links later, access to serial ports on all three. No outside program is started.

## Build order

1. The `registers/` schema, lint rules (a hazard and a range on every writable register), the `device_link` surface kind, and the `scale` field of field types; the simulator with Reset, Inspect, and Frames.
2. Identify, and `read` of a register, through the same engine loop, five runs with one trace signature; a firmware version outside the map stops.
3. The discoverer and drafting rules, against the simulator.
4. `update` with hazard `none`, against the simulator only, with the fault tests: dropped reply, refusal, out of range, link lost after write.
5. `update` and `submit` with `physical`: approvals, `confirm_by`, the safe-state operation and the kill switch.
6. Firmware update against the simulator's slots, including the device that does not come back.
7. The real device, with the owner.

## Open points

- Which protocol first. A register protocol framed over a network link is the likely start: it needs no serial hardware, and a simulator is small.
- Whether a second person is required, not optional, for `physical` in a production environment.
- How fast a person is alerted when a device does not come back, and through which channel.
- Whether the identity check before acting becomes a kernel precondition for every pack.
- Whether `scale` belongs in field types, as here, or in the map beside `data`.
