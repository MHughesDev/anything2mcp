# 310. The device pack

The device pack lets a target be a device reached through its own interface: registers or objects on a fieldbus, or a console on a serial line or a raw socket. It is the device access kind of [section 201](../2-kernel/201-access-kinds-and-modes.md): exchanging frames with hardware over a bus or a link. It brings one mode, `device`, registered when the pack was built ([decision 0107](../../decisions/0107-the-device-mode-and-the-device-pack-as-building-it-settled-it.md)), as item 8 of the entry bar requires; the same record lists where building changed this section. No device pack is built until the command pack has tested the kernel boundary ([decision 0054](../../decisions/0054-devices-through-their-own-interfaces-and-firmware-through-the-devices-own-update.md)). Built today: partly; [section 704](../7-project/704-readiness.md) gives the detail.

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

| Route | Access kind | Served by | Read-back means | Worst outcome | Stance |
| --- | --- | --- | --- | --- | --- |
| The device's management protocol over a network | Request | The request packs: HTTP ([section 303](303-the-http-pack.md)), or a later mode of the request access kind | Read the same object after the change is committed | Locking oneself out by changing the network or the credentials in use | Preferred |
| A console reached by starting a program, such as a remote session client | Command | The command pack ([section 306](306-the-command-pack.md)) | A declared query command | As for a console | Allowed, with a closed list of commands |
| A console on a serial line or a raw socket, with no program between | Device | This pack | A declared query command, its answer parsed by a strict pattern | A saved setting that stops the device starting; nothing protects the link from noise | Allowed, with a closed list of commands |
| Registers or objects on a fieldbus | Device | This pack | Read the register again | Many writes are commands with nothing to read; some move machinery | Allowed for readable registers; the rest is `irreversible` with a hazard |
| The device's own update mechanism | Request or Device | The pack whose protocol defines it | The version and digest the device reports after it restarts | A device that no longer starts, where it has one slot; counters that only go up, even on success | Allowed, under the rules below |
| A debug probe | Not designed | Nothing | Reading memory back | Permanent lock or erase through protection settings; halting a controller that is driving something | Refused until designed, and then never on a device in service |

For a device reached through the request or command packs, this pack adds nothing in code. The rules below apply through the kernel's vocabulary: hazards, approvals, serialized use, a declared safe state. A console on a link is a framed link whose frames are lines of text: one declared command out, one answer in, parsed by a strict pattern.

## Rules for any device operation

These hold for every pack that operates a device.

1. **Exclusive use of the link.** One run at a time on one device link, as desktop work is serialized.
2. **Identify before acting.** The device must report an identity that matches the target's: model, firmware version, and the serial the target declares for the unit. A device that does not match is the class `unknown_variant`, for reads as well as writes (below). Nothing is written to a device whose serial the target does not declare.
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
| Surface kinds | `device_link`: a network address and port, or a serial port with its settings; the protocol; the unit address on a shared bus, from 0 to 255; and `serials`, the serial of the device that answers in each environment, by the environment's name. Serial ports are not built: the address is a socket address, and the one protocol is `modbus_tcp` |
| Modes | `device`, access kind device, with the verbs `read`, `update`, and `submit` |
| Step kinds | `open`, `build_frame`, `write_frame`, `read_frame`, `expect`, checked names within the mode |
| Driver | One |
| File kinds | `registers/`, one file per device model and map |
| Binding fields | `device_map`, `verb`, and `registers`, a map of the one register written to its expression; `read` and `read_back`, maps of an output to the register it is read from |
| Field types | No change. `scale` is on the register in the map, beside `data` |
| Discoverers and drafting rules | Below |

## The file kind: `registers/`

One file per device model and map. It holds every address the pack may touch, how each is encoded, whether it may be written, its safe range, and its hazard. Units live in field types, as rule eight requires; the scale from a raw value is the wire encoding, and is on the register beside `data`. An operation names a register, never an address. A device with an object dictionary instead of a register table uses the same file kind, with `index` and `subindex` as the address; an object dictionary is not built.

```yaml
# types.yaml (kernel file), the field types the map uses. The kernel's
# measurement units are a closed list of lengths and weights, so a speed
# is an exact number with its places.
pump_speed_rpm: {base: decimal, places: 1}
valve_state:    {base: choice, codes: {"0": closed, "1": open}}
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
    scale: "0.1"
    type: pump_speed_rpm
    access: read
  pump_speed_setpoint:
    address: {table: holding, offset: 100}
    data: u16
    scale: "0.1"
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
  holds: {pump_speed_setpoint: "0", inlet_valve: "0"}   # a choice by its stored code
on_link_loss: {device_reverts_to_safe_state: true, after: 2s}
```

| Field | Meaning |
| --- | --- |
| `firmware` | The versions this map was proven against. Any other version stops, reads included |
| `identify` | What the first `open` step of a binding reads to compare the device with the map. It must list `model` and `firmware_version`, and every field it lists must be reported |
| `pace` | The gap between frames, the time a reply must come within, and the most frames per second |
| `address`, `data` | Where a value is and how its bits are laid out: width, signedness, word order (`word_order`), and for text the number of `words`. The wire encoding; it belongs to the map |
| `scale` | The value of one raw step. A raw value is turned into a decimal by its scale, exactly. Part of the wire encoding, so it is in the map |
| `type` | A field type: the kind of value and its places. A float register is to be decoded exactly and rounded to the declared places before any comparison; float registers are not built |
| `access` | `read`, `write`, or `read_write`. A `write` register has nothing to read: a command such as "start" or "reset" |
| `range` | Required on every writable register that holds a number. The safe range in the register's unit, after its scale; each bound a whole number of the register's steps that its data can hold |
| `hazard` | Required on every writable register. `none` is allowed only where the developer declares it |
| `confirm_by` | A sensor register that shows the world followed, with a tolerance and a settle time. Compiled and checked to name a readable register; nothing acts on it yet |
| `read_effect` | `none` by default; `clears` or `advances` for registers whose reading changes them |
| `safe_state`, `on_link_loss` | What safe means for this device, the operation that reaches it, and what the device itself does when the link drops. Compiled into the compiled target; nothing acts on them yet |

## Verbs and how they lower

| Verb | Effect class | Lowers to |
| --- | --- | --- |
| `read` | `read` | `open` with identify, then `write_frame` a read request, `read_frame`, `expect` the reply's form |
| `update` | `write` or `irreversible`, by the hazard table below | Identify; read the current value; `build_frame`: check the range, build the frame, and give its bytes out to be recorded; `write_frame` that frame; `read_frame` the acknowledgement; read the register again |
| `submit` | `irreversible` | Identify; `build_frame`, then `write_frame`, a command to a `write` register or a declared console command; `read_frame` the acknowledgement; `confirm_by` where declared. Console commands and `confirm_by` are not built |

**The bytes of a write are recorded before they are sent** ([decision 0137](../../decisions/0137-a-device-is-held-to-its-serial-a-write-frame-is-recorded-before-it-is-sent-and-a-run-has-a-frame-ceiling.md)). `build_frame` sends nothing and commits nothing. It gives the frame out for reconcile, as hexadecimal, and keeps it as evidence with its digest; the engine stores what a step before the committing one gives for reconcile before it calls the committing step. `write_frame` then sends that frame and no other: a write frame that was not built and given out first, or whose value changed since, is refused unsent. A write whose link died therefore leaves a record of exactly what was sent.

A sequence of device writes is a pipeline of these operations, one commit point each. A single frame that writes several registers at once is one commit point, read back register by register; such a frame is not built, and a binding writes one register (`device.one_register_per_write`).

## The driver, part by part

| Part | What the driver does |
| --- | --- |
| Prepare | Opens the link, or takes it from the supervisor, with exclusive use for this run. Reads the device's identity and compares it with the map, model and firmware version, and with the serial the surface declares for the run's environment. As built, prepare opens the link itself, and the identity is read by the `open` step every binding starts with, once for each map on a link; a step that names a register of a map the device was not compared with sends nothing |
| Act | Sends one frame or one declared console command, built from the map and typed values. Nothing else is sent in that step |
| Confirm | The device's acknowledgement: an echo of what was written, or a reply of the declared form, within `reply_within` |
| Read back | Reads the register again. A register with a `read_effect` is never read back; a write to it is `irreversible` |

**The link sets the pace.** A link carries one request at a time. The driver waits for each reply, keeps `min_gap` between frames, and never exceeds `max_rate`. Where the map gives no pace, a conservative default applies: one frame at a time and a few per second. As built, the default is 250 ms between frames, four a second at most, and a setting can lower the gap. The step's time limit is the map's `reply_within`, and the run's budget counts frames, not only time. As built, the count is the pack's own: one run sends at most `max_frames_per_run` frames on one link, one hundred by default, and stops past it with `policy`, `frame_budget_exceeded`. A write is not started when the frames left would not cover it and its read-back ([decision 0137](../../decisions/0137-a-device-is-held-to-its-serial-a-write-frame-is-recorded-before-it-is-sent-and-a-run-has-a-frame-ceiling.md)).

**A lost link after a write** means the frame may have been applied. It is `possible_commit`. On reconnection, reconcile identifies the device first, then reads the register. If the map declares that the device reverts on link loss, the value read is compared with the safe state as well as with the intended and the earlier value, and a reverted value is reported as such, not as a mismatch. The comparison with the safe state is not built; the value before a write is kept for reconcile. A `submit` has nothing to read, so it stops for a person.

## Identity

**A record is a register or an object,** found by its name in the map. Identity is `natural`, always. A command has no record, and is identity `none`.

**A device is identified by what it reports:** identifier, model, firmware version. That is the subject of every finding and every piece of evidence ([section 218](../2-kernel/218-discovery-and-findings.md)). It is compared with the map before any frame that is not identification, by the `open` step every binding starts with.

**The identifier is the unit's serial, and the surface declares it** for each environment, under `serials` ([decision 0137](../../decisions/0137-a-device-is-held-to-its-serial-a-write-frame-is-recorded-before-it-is-sent-and-a-run-has-a-frame-ceiling.md)). A map is written for a model; the surface says which unit answers where. A device that reports another serial than the one declared for the run's environment is `unknown_variant`, `device_identity_mismatch`, for reads as well as writes: a map proven on one unit does not drive another of the same model at the same address. Where no serial is declared for the environment, a read goes on by model and firmware, and a write stops unsent with `policy`, `device_serial_not_declared`. The compiler refuses a binding that writes through a surface that declares no serial at all (`device.write_without_serial`).

**A firmware version outside the map stops reads too.** A device that does not match is `unknown_variant`, `device_identity_mismatch`, for reads as well as writes, because a register's meaning may change with firmware. This is stricter than the general rule of [section 219](../2-kernel/219-the-evidence-ladder.md), under which a version that is already current keeps running until a canary fails. For a device, a new firmware version drops the bindings on that surface to `reconstructed`, and nothing runs on them until the map's list names the new version. The level does not stop the run: each run stops at its `open` step, which checks the firmware version, and lowering the bindings' level is not built.

Devices reached through the request and command packs need the same identity check before acting. It has no kernel home yet; it is proposed as a precondition the kernel offers to every pack.

## Effects and hazards

Hazards are the centre of this pack.

| Register | Effect class | Hazard | Least approval |
| --- | --- | --- | --- |
| Readable, writable, hazard `none` declared by the developer | `write` | `none` | As policy says |
| Readable, writable, settings that keep the device working | `irreversible` | `device_function` | A person, every run; no standing approval; no pre-approval |
| Readable, writable, moving or heating or opening something | `irreversible` | `physical` | A person, every run; no standing or pre-approval; a second person where policy asks |
| A command register or console command | `irreversible` | As declared; `none` is refused for these | As the hazard says |

An operation with a hazard other than `none` is `irreversible` ([section 213](../2-kernel/213-effects-hazards-and-consent.md)). A read-back still runs and is reported as its own fact, matched or not, but it does not make the write resumable. As built, an operation that writes a register with the hazard `none` is declared `irreversible` too, and still reads it back: the kernel takes a record key only from an input, and a register is found by its name in the map ([decision 0107](../../decisions/0107-the-device-mode-and-the-device-pack-as-building-it-settled-it.md)).

- **Every writable register carries a hazard,** and the compiler refuses one without. The drafter never writes one.
- **Range is enforced before a frame is sent, twice,** by the driver, before it builds the frame ([decision 0106](../../decisions/0106-the-device-pack-speaks-its-first-protocol-itself-and-checks-a-range-before-a-frame-is-built.md)). First the value in the register's unit, against the map's range. Then the raw number it encodes to, against the same range converted by the register's scale and against what the register's bits can hold, so no rounding or overflow can carry a value outside it. A value that is not a whole number of the register's steps is refused, never rounded. Each refusal is `value_out_of_range`, and nothing is sent. The kernel cannot check the range before the preview: its field types carry no range, and the range lives in the map.
- **There is no automatic retry** of any device write, whatever its class. The pack declares its writes not idempotent to the engine, so `transient` never repeats them. A device reached over noise or a shared bus cannot prove that a frame with no reply did nothing.
- **A `physical` operation declares `confirm_by`** where the device has a sensor for it. The setpoint read back proves the device took the value; the sensor within its tolerance after its settle time proves the world followed. They are two facts, reported separately. Not built: until `confirm_by` and the safe-state operation are, the compiler refuses every write to a `physical` register (`device.physical_not_built`), and the driver refuses a step that would send one, whatever compiled target it is handed (`unknown_variant`, `physical_not_built`).

**A safe state when a run stops halfway.** A pipeline that opens a valve and then starts a pump may stop after the first. The engine does not continue it, undo it, or send anything further: a stop means the device or the link is in a state with no rule, and more frames into that state is a guess. Instead the stop is recorded with what landed and what may have, a person is alerted, and the alert offers the declared safe-state operation as one action. The person invoking it is the approval it needs. The kill switch reaches the link and runs the same operation. The device's own fail-safe, declared as `on_link_loss`, is the first line; the safe-state operation is the second (hypothesis H-34). The alert, the safe-state operation, and the kill switch reaching the link are not built.

| Effect in the manifest | Declared as |
| --- | --- |
| Uses the network or a serial port | The declared links |
| Holds a session | For surfaces of kind `device_link` |
| Changes a device | The declared links, with exclusive use |

All are `enforced`.

## Sessions and secrets

| Rule | How |
| --- | --- |
| One link, one session | The open link is the session's live state, held on the home runner by the supervisor ([section 204](../2-kernel/204-supervising-subsystems.md)). As built, the driver opens the link in prepare and closes it at release; no link is kept for another run |
| No sharing between runs | One run holds the link at a time, through a lock the scheduler grants. A second run waits or gets `session_in_use`. Each run identifies the device again |
| Credentials and keys | In the vault, injected when the link opens. Never in a frame the trace keeps; frames are stored with secrets removed by structure. Not built: the first protocol has no sign-in, and the driver uses no lease. A step keeps the frame it sent, as hexadecimal, and only the length of the reply; `build_frame` keeps the frame of a write, with its digest, before it is sent |
| Links with no sign-in | Many fieldbus protocols have none. Then the session has no identity, and consent carries the weight: the person names the device ([section 213](../2-kernel/213-effects-hazards-and-consent.md)), the surface is declared, and nothing else is reachable ([section 215](../2-kernel/215-sessions-and-the-session-keeper.md)) |

## Firmware

Built, against the simulated device only ([decision 0316](../../decisions/0316-firmware-is-inspected-by-an-optional-provider-and-updated-by-one-operation-through-the-devices-own-sequence.md)): inspection of an image given as a file, and the update through the device's own register sequence. Not built: what a person sees to approve an update, an image as a stored blob in place of hexadecimal text, inspection of an image the device offers, a signature check, and an update through the request pack.

**Inspection is discovery only.** An optional provider, off by default, reads an image the person supplies as a file, or one the device offers through its own interface for that purpose. The subject of every finding is the image, by digest. What it finds is `inferred`: a setting named in an image is a claim about a device, not an observation of one. It lists regions, files, and settings, and decrypts nothing and recovers no keys. The image is never changed. An image that is encrypted or protected is a named stop: `edge_blocked`, `image_protected`. As built, the optional provider is `device_firmware`, in the device pack's crate with a manifest of its own, and it is off until a product registers it. It reads one file handed under the name `firmware_image`, sends no frame, and makes one finding of the kind `firmware_image` with the source `artifact`: the image's form, version, regions with their offsets and lengths, whether its checksum holds, and the settings its payload states, with the value of a setting named as a secret left out. An image of a form it does not read makes no finding; one that looks encrypted is the named stop, by a heuristic that decrypts nothing.

**An update goes only through the device's own documented mechanism.** Where the fieldbus protocol defines one, this pack carries it; where the device's management protocol does, the request pack does, under the same rules. An update is one operation with one commit point, and the strictest handling in the design. It is always `irreversible`, with the hazard `device_function`. As built, a map declares the mechanism under `firmware_update` (the mechanism `register_blocks`: the offsets of a command register, a length register, and one block address, the registers that report the update's state, its error, and the device's checksum, and the window in which the device must come back), and an `update` binding names it with a `firmware` field: the image, the version it carries, the recovery route, what cannot be undone, whether the device checks the maker's signature, the preconditions, and optionally the digests it accepts. The offsets the update writes are named by no register, so no other operation can write them. One step commits and carries the whole sequence: the length, update mode, each block, verify, a read of the state, and the commit command, whose reply the confirm steps read.

| Stage | What happens |
| --- | --- |
| Before publishing | The operation declares its recovery route, what cannot be undone (counters that only rise), the preconditions the device's documentation sets, and whether the device checks the maker's signature. The compiler refuses a firmware operation without a recovery route |
| Precondition | Identify; the current version; each declared precondition read from the device, such as power, free space, a second slot |
| The image | A file input, stored as a blob, its digest recorded. It must be an image the device's maker issued for that model. The target may list the digests it accepts; one not listed stops with `input` |
| Approval | A person, every run, shown the current and the proposed version, the image's digest, whether the device checks its signature, and what cannot be undone |
| Act and confirm | The mechanism, and nothing else. The device accepts the image and checks it |
| Read back | After a declared restart window: identify again, and compare the version and the device's own checksum of its image. A device reports a checksum of its own kind, not the digest recorded above: the simulated device reports a CRC-32 |
| If it does not come back | `possible_commit`, `device_not_returned`. The run stops, a person is alerted, and the declared recovery route is shown. Nothing is retried |

The pack never builds, patches, or signs an image, and never checks a signature in the device's place.

## Discovery and drafting

| It reads | Finding | Level |
| --- | --- | --- |
| What the device answers when asked to identify itself | The device subject: identifier, model, firmware | `discovered` |
| A listing the device computes of its objects, where the protocol offers one | Objects, with access and data type | `discovered` |
| A published map of registers or objects, as a file | Registers, with addresses, types, and units where stated | `inferred` |
| A firmware image, through the optional provider | Regions and settings | `inferred` |

**Discovery cannot write, and here it cannot even read freely.** A device is only asked to identify and list itself. The discoverer reads only addresses the device lists or a published map names, and never one marked or described as changing on read. The scout tries the identification read, and reads of objects the device itself lists as readable. On a register table that lists nothing it tries only identification; the rest waits for the developer's `draft.try`. As built, the first and third rows of the table above are: the first protocol offers no listing, so the discoverer `device_identity` sends one identification request and nothing else, and the discoverer `device_register_map` reads a published map given as a file in the project, in the layout the [pack's design](../../architecture/crates/a2m-pack-device.md) states, sends no frame, and makes one finding a file at `inferred`, source `published_description` ([decision 0188](../../decisions/0188-credentials-over-plain-links-drafted-workbooks-and-published-device-maps.md)). The driver holds the same line by itself: handed the discovery posture, it refuses every frame but identification with `policy`, `discovery_identifies_only`.

The drafter writes a map of readable registers with their data types, and one `read` operation for each. It writes no write, no range, no hazard, no safe state, and no unit the device or its map did not state. As built, from identification it writes no map, and the drafter's report says why: a map with no register is nothing to try, and would stand beside the map the device's documentation gives ([decision 0441](../../decisions/0441-a-device-draft-states-its-places-drafts-no-empty-map-and-its-update-fixture-is-documented.md)). From a published map it writes a map of each input or holding register whose data type the map states, each `access: read`, with the model and firmware versions the map states (a question where it states none), and one read of each; a number whose unit the map states is read as an exact number and its description gives the unit as the map's, because a target's field type is not the drafter's to write; a scaled number's description also gives the places its scale gives, one for `0.1` and two for `0.01`; one whose unit it does not state holds that question in its read alone. The map file is hostile input: it is bounded in size, rows, and cells, and a unit, a model, or a version reaches a drafted file only from a closed set of characters, or else only quoted apart in a question ([decision 0197](../../decisions/0197-no-client-takes-a-proxy-from-the-environment-and-stand-only-protocols.md)). A coil, a discrete input, a register that can only be written, a row with no data type, and one whose address the protocol cannot hold give no register and no read, and the describer names each by its line and why.

**A register named as a secret is never drafted.** A row of a published map whose name the compiler takes for a secret's is left out of the drafted map and has no read; the pack's description of the map says so ([decision 0302](../../decisions/0302-a-field-named-as-a-secret-is-never-drafted-as-an-input-or-an-output.md)).

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
| `input` | `payload_unreadable` | The compiled target was compiled by another build of the pack; nothing is sent |
| `policy` | `surface_not_declared`, `approval_missing` | A link not declared; a hazard without its approval. `approval_missing` is the policy engine's, not the pack's |
| `policy` | `device_serial_not_declared`, `discovery_identifies_only`, `frame_budget_exceeded` | A write where the surface declares no serial for the environment; a frame other than identification during discovery; a frame past the run's ceiling. Nothing is sent |
| `busy` | `session_in_use` | Another run holds the link. The scheduler raises it, not the pack |
| `unknown_variant` | `device_identity_mismatch`, `unexpected_reply` | Model or firmware not in the map, or another serial than the surface declares; a reply of no declared form |
| `unknown_variant` | `physical_not_built` | A step that writes a `physical` register reached the driver; nothing is sent |
| `rejected` | `device_refused` | The device answers with an exception: an address or a value it will not take |
| `edge_blocked` | `device_protected`, `image_protected` | A protection on the device or the image refuses. `image_protected` is raised by the optional provider for an image that looks encrypted. `device_protected` is not yet raised: there is no protection check on a device |
| `transient` | `timeout`, `connection_lost`, `frame_corrupt` | On a read, or before a write frame was sent |
| `transient` | `device_busy` | The device answered that it is busy, on a read or a write |
| `possible_commit` | `no_confirmation`, `device_not_returned` | A write frame was sent and no clear acknowledgement came; an update was committed and the device did not come back in its window |
| `input` | `image_not_accepted` | An update's image is not hexadecimal text, not of the form an image has, damaged, too large, of another version than the operation names, or not on its list of digests. Nothing is sent |
| `policy` | `update_precondition_unmet` | A condition the update declares, or the device's own update state, does not hold. Nothing is written |
| `rejected` | `image_refused` | The device refused the image or a block after update mode began; it was told to leave update mode, and its other slot's image was given up |
| `mismatch` | `image_checksum_differs` | After an update the device's checksum of its image is not the one the image carries |
| `mismatch` | `read_back_differs`, `sensor_out_of_tolerance` | The register or the declared sensor disagrees. `read_back_differs` is the engine's comparison, not the pack's. `sensor_out_of_tolerance` is not yet raised: `confirm_by` is not built |

A device may also answer that it is busy: `device_busy`. On a read that is `transient`. On a write it is never retried, like every write in this pack.

The codes the pack raises are declared in its manifest and listed in its [design](../../architecture/crates/a2m-pack-device.md); the codes above that it does not raise are not declared.

## The test stand

A device pack is proven first against an emulated device. Here that is a simulated device in software, in the test application family, on a local port. It exists at `targets/example-device` ([README](../../../targets/example-device/README.md)) and speaks a subset of the Modbus application protocol over TCP. No real hardware runs in CI. It holds registers, a small simulated process (a pump whose measured speed follows its setpoint with a lag), and an update mechanism with two slots.

| Control call | Reports or does |
| --- | --- |
| Reset, Inspect | Restore known state; return every register and the state of each simulated output |
| Frames | Every frame received, decoded, in order, with how each was answered |
| Count writes | How many times each register was written |
| Faults | Apply a write and drop the reply; delay; corrupt a frame; refuse with an exception; report another firmware version; revert on link loss; restart and stay down; refuse an image |

Against the simulator, read-back is exact, a snapshot undoes anything, and ground truth is complete. That shows the pack is right. It does not show that a real device behaves the same. A real device model enters a target as a test environment first, with settings that are safe to write, and the gates run there before any production use. Before support for any protocol is claimed, one real device model is tried that way, at the end, with the owner, and with the owner's go-ahead in that session (rule ten).

## What it needs from outside

No client crate for the first protocol: the pack speaks its subset of the Modbus application protocol itself, in one module, with no code shared with the test stand ([decision 0106](../../decisions/0106-the-device-pack-speaks-its-first-protocol-itself-and-checks-a-range-before-a-frame-is-built.md)). Its outside crates are `serde`, `serde_json`, and Tokio. For serial links later, an outside crate for access to serial ports on all three operating systems, to be verified. No outside program is started.

## The entry bar

The eight items of [section 202](../2-kernel/202-kernel-and-packs.md).

| # | Item | The device pack's answer |
| --- | --- | --- |
| 1 | A driver, no retries inside | The `device` driver, with no automatic retry of any write, even as `transient` |
| 2 | Read-back for every write | Read the register again. A `write` register, a console command, or a register with a `read_effect` has none, and its write is `irreversible`. Any hazard other than `none` makes a write `irreversible` whatever its read-back |
| 3 | Each identity strategy | `natural`, by the register's or object's name in the map. A command is `none`. `composite`, `stamped`, and `find` are refused with a reason |
| 4 | A test stand | The simulated device above, with its control interface |
| 5 | A discoverer | Identification, the device's own listing, a published map, and firmware inspection through the optional provider. Identification, a published map, and firmware inspection are built |
| 6 | Failures mapped | The table above |
| 7 | Effects and hazards | The tables above. Every writable register carries a hazard and a range |
| 8 | A record per new mode | [Decision 0107](../../decisions/0107-the-device-mode-and-the-device-pack-as-building-it-settled-it.md) |

## Build order

1. The `registers/` schema, its lint rules (a hazard and a range on every writable register), the `device_link` surface kind, and `scale` on the register; the simulator with Reset, Inspect, and Frames.
2. Identify, and `read` of a register, through the same engine loop, five runs with one trace signature; a firmware version outside the map stops.
3. The discoverer and drafting rules, against the simulator.
4. `update` with hazard `none`, against the simulator only, with the fault tests: dropped reply, refusal, out of range, link lost after write.
5. `update` and `submit` with `physical`: approvals, `confirm_by`, the safe-state operation, and the kill switch.
6. Firmware update against the simulator's slots, including the device that does not come back.
7. The real device, with the owner.

Steps 1, 3, and 4 are built in `a2m-pack-device`, except what the [crate's design](../../architecture/crates/a2m-pack-device.md) lists as not built. Step 2 is built through the driver's handle: its five runs give one signature there, and the engine loop has not run the mode. Step 6 is built in `a2m-pack-device` as decision 0316 states it. Steps 5 and 7 are not built.

## Open points

- Which protocol first. Settled for the test stand and the pack: a subset of the Modbus application protocol over TCP, a register protocol framed over a network link, which needs no serial hardware ([decision 0106](../../decisions/0106-the-device-pack-speaks-its-first-protocol-itself-and-checks-a-range-before-a-frame-is-built.md)). A management protocol over a network, through a request pack, is the safest start of all.
- Whether emulators for whole devices, beyond a protocol-level test stand, are worth the weight.
- Whether a second person is required, not optional, for `physical`, at least in a production environment.
- How fast a person is alerted when a device does not come back, and through which channel.
- Whether the identity check before acting becomes a kernel precondition for every pack.
- Whether `scale` belongs in field types or in the map beside `data`. Settled: in the map, as the wire encoding ([decision 0107](../../decisions/0107-the-device-mode-and-the-device-pack-as-building-it-settled-it.md)).
- The exact preconditions a `device_function` operation must declare; this section starts the list.
