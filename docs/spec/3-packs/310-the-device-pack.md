# 310. The device pack

The device pack lets a target be a device reached through its own interface: registers or objects on a fieldbus, or a console on a serial line or a raw socket. It is the device archetype of [section 201](../2-kernel/201-archetypes-and-modes.md): exchanging frames with hardware over a bus or a link. It brings one mode, `device`, which is proposed: it becomes real by its own decision record when the pack is built, as item 8 of the entry bar requires. No device pack is built until the command pack has tested the kernel boundary ([decision 0054](../../decisions/0054-devices-through-their-own-interfaces-and-firmware-through-the-devices-own-update.md)).

This section holds the rules for any device operation, whichever pack carries it out, and the design of the pack under those rules. A device is the place where a write can move something in the world, and everything here is arranged around that. Where the engine loop fits a device well, this section says so; where it strains, it says so too.

## The short answer

| Question | Answer |
| --- | --- |
| Can a device be operated through verified tools? | Yes, where the device offers an interface of its own that can be read as well as written: a management protocol, a console with known commands, registers on a fieldbus |
| Can firmware be inspected? | Yes, as discovery. It yields findings and never an operation. An optional provider does it |
| Can firmware be updated? | Yes, only through the device's own documented update mechanism, as an `irreversible` operation with the hazard `device_function` |
| Can firmware be changed in other ways, or protections be got around? | No |

The REA project, studied for this ([research note 16](../../research/16-rea-methods.md)), is careful and entirely passive at its lowest levels. It parses firmware containers and images, device trees, kernel modules, and saved logic-analyzer and packet recordings, all from files. It writes nothing to any device, speaks to no bus and no debug probe, and runs nothing under emulation; its only changes are a few tools for a phone or emulator attached for debugging, each with a fixed argument list. Where it does change something it shows the right habits: take the bytes first, probe before writing, stop on an unclear probe, read back and report the result as its own fact. It is a way to understand a device's software, not to operate a device, and nothing there is reused for that.

## Ways to reach a device, best first

| Route | Archetype | Served by | Read-back means | Worst outcome | Stance |
| --- | --- | --- | --- | --- | --- |
| The device's management protocol over a network | Request | The request packs: HTTP ([section 303](303-the-http-pack.md)), or a later mode of the request archetype | Read the same object after the change is committed | Locking oneself out by changing the network or the credentials in use | Preferred |
| A console reached by starting a program, such as a remote session client | Command | The command pack ([section 306](306-the-command-pack.md)) | A declared query command | As for a console | Allowed, with a closed list of commands |
| A console on a serial line or a raw socket, with no program between | Device | This pack | A declared query command, its answer parsed by a strict pattern | A saved setting that stops the device starting; nothing protects the link from noise | Allowed, with a closed list of commands |
| Registers or objects on a fieldbus | Device | This pack | Read the register again | Many writes are commands with nothing to read; some move machinery | Allowed for readable registers; the rest is `irreversible` with a hazard |
| The device's own update mechanism | Request or Device | The pack whose protocol defines it | The version and digest the device reports after it restarts | A device that no longer starts, where it has one slot; counters that only go up, even on success | Allowed, under the rules below |
| A debug probe | Not designed | Nothing | Reading memory back | Permanent lock or erase through protection settings; halting a controller that is driving something | Refused until designed, and then never on a device in service |

For a device reached through the request or command packs, this pack adds nothing in code. The rules below apply through the kernel's vocabulary: hazards, approvals, serialized use, a declared safe state. A console on a link is a framed link whose frames are lines of text: one declared command out, one answer in, parsed by a strict pattern.

## Rules for any device operation

These hold for every pack that operates a device.

1. **Exclusive use of the link.** One run at a time on one device link, as desktop work is serialized.
2. **Identify before acting.** The device must report an identity that matches the target's: model and firmware version. A device that does not match is the class `unknown_variant`, for reads as well as writes (below).
3. **Probe, then write, then read back.** The four habits of [section 213](../2-kernel/213-effects-hazards-and-consent.md): take the bytes to be sent first and record their digest; probe the state and treat an unclear probe as a stop; report the read-back as its own fact; set a safety setting, read it back, and go on only if it took.
4. **No automatic retry of a device write.** A write that timed out is `possible_commit`, and reconcile reads the device.
5. **A hazard on every write** that could affect the device's ability to work or anything physical, with the approvals section 213 requires.
6. **A declared safe state and a stop.** For anything physical, the target declares the state that is safe and the operation that reaches it. The kill switch reaches the device link.
7. **Rate limits from the device's own documentation,** or a conservative default.
8. **Never the link we are standing on.** An operation that changes the address, the credentials, or the interface it is itself using is refused unless the target declares how the session is found again afterwards.

## What this pack reaches, and what it refuses

| Reaches | How |
| --- | --- |
| Registers or objects on a fieldbus | By address, from a declared map, over a network link or later a serial port |
| A console on a serial line or a raw socket | One declared command at a time, its answer parsed by a strict pattern |

| Refused | Why |
| --- | --- |
| A debug probe, for reading or writing | Not designed; the worst outcome is permanent |
| An exploit, an unlock, rooting, or a way into a device it does not offer | Rule five |
| Defeating secure start-up, a signature check, or read-out protection | Rule five. A protection that refuses is a named stop |
| Writing memory, or any address not in a declared map | Not an interface the device offers for the purpose |
| Scanning addresses to see what answers | A read is not always without effect on a device (below) |
| A free-form console line from a caller | Commands are not a shell ([section 213](../2-kernel/213-effects-hazards-and-consent.md)) |
| Building, patching, or re-signing a firmware image | It is not an operation on an existing system; it makes a new one |
| Changing settings that only go one way, such as fuses, unless the device's maker documents the operation and the target declares it | It can never be read back into the previous state |
| Any operation on a real device without a test environment first | Rule ten, applied to hardware |

## What it registers

| Registration | Content |
| --- | --- |
| Surface kinds | `device_link`: a network address and port, or a serial port with its settings; the protocol; and the unit address on a shared bus |
| Modes | `device`, archetype device, proposed |
| Step kinds | `open`, `write_frame`, `read_frame`, `expect`, checked names within the mode |
| Driver | One |
| File kinds | `registers/`, one file per device model and map |
| Binding fields | `device_map`, `verb`, and `registers`, a map of register to expression |
| Field types | The kernel's `types.yaml` gains `scale` |
| Discoverers and drafting rules | Below |

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
| `firmware` | The versions this map was proven against. Any other version stops, reads included |
| `identify` | What prepare reads to compare the device with the map |
| `pace` | The gap between frames, the time a reply must come within, and the most frames per second |
| `address`, `data` | Where a value is and how its bits are laid out: width, signedness, word order. The wire encoding; it belongs to the map |
| `type` | A field type: unit, scale from the raw value, places. A raw value is turned into a decimal by its scale; a float register is decoded exactly and rounded to the declared places before any comparison |
| `access` | `read`, `write`, or `read_write`. A `write` register has nothing to read: a command such as "start" or "reset" |
| `range` | Required on every writable register that holds a number. The safe range in the field type's unit |
| `hazard` | Required on every writable register. `none` is allowed only where the developer declares it |
| `confirm_by` | A sensor register that shows the world followed, with a tolerance and a settle time |
| `read_effect` | `none` by default; `clears` or `advances` for registers whose reading changes them |
| `safe_state`, `on_link_loss` | What safe means for this device, the operation that reaches it, and what the device itself does when the link drops |

## Verbs and how they lower

| Verb | Effect class | Lowers to |
| --- | --- | --- |
| `read` | `read` | `open` with identify, then `write_frame` a read request, `read_frame`, `expect` the reply's form |
| `update` | `write` or `irreversible`, by the hazard table below | Identify; read the current value; check the range; `write_frame`; `read_frame` the acknowledgement; read the register again |
| `submit` | `irreversible` | Identify; `write_frame` a command to a `write` register or a declared console command; `read_frame` the acknowledgement; `confirm_by` where declared |

A sequence of device writes is a pipeline of these operations, one commit point each. A single frame that writes several registers at once is one commit point, read back register by register.

## The driver, part by part

| Part | What the driver does |
| --- | --- |
| Prepare | Opens the link, or takes it from the supervisor, with exclusive use for this run. Reads the device's identity and compares it with the map: model and firmware version |
| Act | Sends one frame or one declared console command, built from the map and typed values. Nothing else is sent in that step |
| Confirm | The device's acknowledgement: an echo of what was written, or a reply of the declared form, within `reply_within` |
| Read back | Reads the register again. A register with a `read_effect` is never read back; a write to it is `irreversible` |

**The link sets the pace.** A link carries one request at a time. The driver waits for each reply, keeps `min_gap` between frames, and never exceeds `max_rate`. Where the map gives no pace, a conservative default applies: one frame at a time and a few per second. The step's time limit is the map's `reply_within`, and the run's budget counts frames, not only time.

**A lost link after a write** means the frame may have been applied. It is `possible_commit`. On reconnection, reconcile identifies the device first, then reads the register. If the map declares that the device reverts on link loss, the value read is compared with the safe state as well as with the intended and the earlier value, and a reverted value is reported as such, not as a mismatch. A `submit` has nothing to read, so it stops for a person.

## Identity

**A record is a register or an object,** found by its name in the map. Identity is `natural`, always. A command has no record, and is identity `none`.

**A device is identified by what it reports:** identifier, model, firmware version. That is the subject of every finding and every piece of evidence ([section 218](../2-kernel/218-discovery-and-findings.md)). Prepare compares it with the map before any frame that is not identification.

**A firmware version outside the map stops reads too.** A device that does not match is `unknown_variant`, `device_identity_mismatch`, for reads as well as writes, because a register's meaning may change with firmware. This is stricter than the general rule of [section 219](../2-kernel/219-the-evidence-ladder.md), under which a version that is already current keeps running until a canary fails. For a device, a new firmware version drops the bindings on that surface to `reconstructed`, and nothing runs on them until the gates pass again with the new version added to the map's list.

Devices reached through the request and command packs need the same identity check before acting. It has no kernel home yet; it is proposed as a precondition the kernel offers to every pack.

## Effects and hazards

Hazards are the centre of this pack.

| Register | Effect class | Hazard | Least approval |
| --- | --- | --- | --- |
| Readable, writable, hazard `none` declared by the developer | `write` | `none` | As policy says |
| Readable, writable, settings that keep the device working | `irreversible` | `device_function` | A person, every run; no standing approval; no pre-approval |
| Readable, writable, moving or heating or opening something | `irreversible` | `physical` | A person, every run; no standing or pre-approval; a second person where policy asks |
| A command register or console command | `irreversible` | As declared; `none` is refused for these | As the hazard says |

An operation with a hazard other than `none` is `irreversible` ([section 213](../2-kernel/213-effects-hazards-and-consent.md)). A read-back still runs and is reported as its own fact, matched or not, but it does not make the write resumable.

- **Every writable register carries a hazard,** and the compiler refuses one without. The drafter never writes one.
- **Range is enforced before a frame is sent, twice.** The kernel validates the input against the field type and the range and stops with `value_out_of_range` before the preview. The driver checks the encoded raw value against the same range, converted by the field type's scale, before it builds the frame, so no rounding or overflow can carry a value outside it.
- **There is no automatic retry** of any device write, whatever its class. The pack declares its writes not idempotent to the engine, so `transient` never repeats them. A device reached over noise or a shared bus cannot prove that a frame with no reply did nothing.
- **A `physical` operation declares `confirm_by`** where the device has a sensor for it. The setpoint read back proves the device took the value; the sensor within its tolerance after its settle time proves the world followed. They are two facts, reported separately.

**A safe state when a run stops halfway.** A pipeline that opens a valve and then starts a pump may stop after the first. The engine does not continue it, undo it, or send anything further: a stop means the device or the link is in a state with no rule, and more frames into that state is a guess. Instead the stop is recorded with what landed and what may have, a person is alerted, and the alert offers the declared safe-state operation as one action. The person invoking it is the approval it needs. The kill switch reaches the link and runs the same operation. The device's own fail-safe, declared as `on_link_loss`, is the first line; the safe-state operation is the second (hypothesis H-34).

| Effect in the manifest | Declared as |
| --- | --- |
| Uses the network or a serial port | The declared links |
| Holds a session | For surfaces of kind `device_link` |
| Changes a device | The declared links, with exclusive use |

All are `enforced`.

## Sessions and secrets

| Rule | How |
| --- | --- |
| One link, one session | The open link is the session's live state, held on the home runner by the supervisor ([section 204](../2-kernel/204-supervising-subsystems.md)) |
| No sharing between runs | One run holds the link at a time, through a lock the scheduler grants. A second run waits or gets `session_in_use`. Each run identifies the device again |
| Credentials and keys | In the vault, injected when the link opens. Never in a frame the trace keeps; frames are stored with secrets removed by structure |
| Links with no sign-in | Many fieldbus protocols have none. Then the session has no identity, and consent carries the weight: the person names the device ([section 213](../2-kernel/213-effects-hazards-and-consent.md)), the surface is declared, and nothing else is reachable ([section 215](../2-kernel/215-sessions-and-the-session-keeper.md)) |

## Firmware

**Inspection is discovery only.** An optional provider, off by default, reads an image the person supplies as a file, or one the device offers through its own interface for that purpose. The subject of every finding is the image, by digest. What it finds is `inferred`: a setting named in an image is a claim about a device, not an observation of one. It lists regions, files, and settings, and decrypts nothing and recovers no keys. The image is never changed. An image that is encrypted or protected is a named stop: `edge_blocked`, `image_protected`.

**An update goes only through the device's own documented mechanism.** Where the fieldbus protocol defines one, this pack carries it; where the device's management protocol does, the request pack does, under the same rules. An update is one operation with one commit point, and the strictest handling in the design. It is always `irreversible`, with the hazard `device_function`.

| Stage | What happens |
| --- | --- |
| Before publishing | The operation declares its recovery route, what cannot be undone (counters that only rise), the preconditions the device's documentation sets, and whether the device checks the maker's signature. The compiler refuses a firmware operation without a recovery route |
| Precondition | Identify; the current version; each declared precondition read from the device, such as power, free space, a second slot |
| The image | A file input, stored as a blob, its digest recorded. It must be an image the device's maker issued for that model. The target may list the digests it accepts; one not listed stops with `input` |
| Approval | A person, every run, shown the current and the proposed version, the image's digest, whether the device checks its signature, and what cannot be undone |
| Act and confirm | The mechanism, and nothing else. The device accepts the image and checks it |
| Read back | After a declared restart window: identify again, and compare the version and digest the device reports |
| If it does not come back | `possible_commit`, `device_not_returned`. The run stops, a person is alerted, and the declared recovery route is shown. Nothing is retried |

The pack never builds, patches, or signs an image, and never checks a signature in the device's place.

## Discovery and drafting

| It reads | Finding | Level |
| --- | --- | --- |
| What the device answers when asked to identify itself | The device subject: identifier, model, firmware | `discovered` |
| A listing the device computes of its objects, where the protocol offers one | Objects, with access and data type | `discovered` |
| A published map of registers or objects, as a file | Registers, with addresses, types, and units where stated | `inferred` |
| A firmware image, through the optional provider | Regions and settings | `inferred` |

**Discovery cannot write, and here it cannot even read freely.** A device is only asked to identify and list itself. The discoverer reads only addresses the device lists or a published map names, and never one marked or described as changing on read. The scout tries the identification read, and reads of objects the device itself lists as readable. On a register table that lists nothing it tries only identification; the rest waits for the developer's `draft.try`.

The drafter writes a map of readable registers with their data types, and one `read` operation for each. It writes no write, no range, no hazard, no safe state, and no unit the device or its map did not state.

## Where the loop fits, and where it strains

| Part | Fit |
| --- | --- |
| Prepare, act, confirm, read back | Fit cleanly: one frame, one acknowledgement, one register read again |
| A read is without effect | Not always. A clear-on-read register breaks the assumption; the map must say so, and such a register is never read back |
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

A device pack is proven first against an emulated device. Here that is a simulated device in software, in the test application family, speaking the chosen protocol on a local port. No real hardware runs in CI. It holds registers, a small simulated process (a pump whose measured speed follows its setpoint with a lag), and an update mechanism with two slots.

| Control call | Reports or does |
| --- | --- |
| Reset, Inspect | Restore known state; return every register and the state of each simulated output |
| Frames | Every frame received, decoded, in order, with how each was answered |
| Count writes | How many times each register was written |
| Faults | Apply a write and drop the reply; delay; corrupt a frame; refuse with an exception; report another firmware version; revert on link loss; restart and stay down; refuse an image |

Against the simulator, read-back is exact, a snapshot undoes anything, and ground truth is complete. That shows the pack is right. It does not show that a real device behaves the same. A real device model enters a target as a test environment first, with settings that are safe to write, and the gates run there before any production use. Before support for any protocol is claimed, one real device model is tried that way, at the end, with the owner, and with the owner's go-ahead in that session (rule ten).

## What it needs from outside

Each an outside crate to be verified: a client for the chosen fieldbus protocol, asynchronous on Tokio, under a licence on the closed list, on all three operating systems; for serial links later, access to serial ports on all three. No outside program is started.

## The entry bar

The eight items of [section 202](../2-kernel/202-kernel-and-packs.md).

| # | Item | The device pack's answer |
| --- | --- | --- |
| 1 | A driver, no retries inside | The `device` driver, with no automatic retry of any write, even as `transient` |
| 2 | Read-back for every write | Read the register again. A `write` register, a console command, or a register with a `read_effect` has none, and its write is `irreversible`. Any hazard other than `none` makes a write `irreversible` whatever its read-back |
| 3 | Each identity strategy | `natural`, by the register's or object's name in the map. A command is `none` |
| 4 | A test stand | The simulated device above, with its control interface |
| 5 | A discoverer | Identification, the device's own listing, a published map, and firmware inspection through the optional provider |
| 6 | Failures mapped | The table above |
| 7 | Effects and hazards | The tables above. Every writable register carries a hazard and a range |
| 8 | A record per new mode | `device` is proposed. It becomes real by its own record when the pack is built |

## Build order

1. The `registers/` schema, its lint rules (a hazard and a range on every writable register), the `device_link` surface kind, and the `scale` field of field types; the simulator with Reset, Inspect, and Frames.
2. Identify, and `read` of a register, through the same engine loop, five runs with one trace signature; a firmware version outside the map stops.
3. The discoverer and drafting rules, against the simulator.
4. `update` with hazard `none`, against the simulator only, with the fault tests: dropped reply, refusal, out of range, link lost after write.
5. `update` and `submit` with `physical`: approvals, `confirm_by`, the safe-state operation, and the kill switch.
6. Firmware update against the simulator's slots, including the device that does not come back.
7. The real device, with the owner.

## Open points

- Which protocol first. A register protocol framed over a network link is the likely start: it needs no serial hardware, and its simulator is small. A management protocol over a network, through a request pack, is the safest start of all.
- Whether emulators for whole devices, beyond a protocol-level test stand, are worth the weight.
- Whether a second person is required, not optional, for `physical`, at least in a production environment.
- How fast a person is alerted when a device does not come back, and through which channel.
- Whether the identity check before acting becomes a kernel precondition for every pack.
- Whether `scale` belongs in field types, as here, or in the map beside `data`.
- The exact preconditions a `device_function` operation must declare; this section starts the list.
