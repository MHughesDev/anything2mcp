# The simulated device

A test stand for the device pack ([section 310](../../docs/spec/3-packs/310-the-device-pack.md)): an imaginary pump controller, reached over a network link, with readable and writable points and its own firmware update mechanism. The device pack is proven against this and never against real hardware. The rules for a stand are in [the architecture](../../docs/architecture/other-parts.md) and the [test application plan](../../docs/plan/test-application.md).

It names no real vendor or product. The vendor it reports is `example`, and the model is `example-controller`.

## Starting it

Rust tests call `example_device::TestDevice::start()`. It starts a device inside the test, bound to port 0 on the loopback address, and stops it when the value is dropped. There is no binary and no fixed port.

```rust
let device = TestDevice::start().await?;          // or start_with(DeviceConfig { .. })
let address = device.address();                   // 127.0.0.1 and the port the system chose
let unit = device.unit();                         // 1 unless the config says otherwise
// ... the code under test opens a link to `address` ...
let control = device.control();                   // ground truth, and the faults
assert_eq!(control.number("pump_speed_setpoint"), Some(12_000));
assert_eq!(control.write_count("pump_speed_setpoint"), 1);
assert!(control.frames().iter().all(|frame| frame.kind == FrameKind::Read));
```

`DeviceConfig` sets the unit identifier (1), the firmware version in the active slot (2.4.0), the version in the standby slot (2.3.0, or `None` for a slot with no valid image), and the pump's ramp per tick (4000 raw, 400.0 rpm).

## The protocol

Section 310 leaves the first protocol open and says a register protocol framed over a network link is the likely start. This stand speaks **the Modbus application protocol over TCP**, because it is the most widely implemented open register protocol, its tables (coils, discrete inputs, holding registers, input registers) are the ones the section's example map already uses, and its frames are small enough to write by hand. The frames are written in `src/protocol.rs` with no protocol library, so the stand shares no code with the client it will be used to test.

The subset, by the standard's names:

| Function | Name | Notes |
| --- | --- | --- |
| 01 | Read Coils | 1 to 2000 bits |
| 02 | Read Discrete Inputs | 1 to 2000 bits |
| 03 | Read Holding Registers | 1 to 125 words |
| 04 | Read Input Registers | 1 to 125 words |
| 05 | Write Single Coil | The value is `0xFF00` or `0x0000`; the reply echoes the request |
| 06 | Write Single Register | The reply echoes the request, even where the device stores something else |
| 16 | Write Multiple Registers | 1 to 123 words. Every value is checked before any is stored: a refused frame stores nothing |
| 43 / 14 | Read Device Identification | Conformity level `0x83`. Objects `0x00` vendor name, `0x01` product code, `0x02` revision, `0x80` serial number, as text. Read codes 1 to 3 as a stream, 4 for one object. Everything fits in one reply |

Framing is the MBAP header: transaction identifier, protocol identifier zero, length, unit identifier. One request is answered before the next is read. A header with another protocol identifier, or a length outside 2 to 254, gets no reply and the link is closed.

Exception replies, by the standard's names:

| Code | Name | When |
| --- | --- | --- |
| 01 | Illegal function | A function outside the subset, or another MEI type |
| 02 | Illegal data address | An address with no point, anywhere in the span a request covers; a write to a read-only point; a write of part of a two-word point; an identification object the device does not have |
| 03 | Illegal data value | A count outside the standard's limits; a coil value that is neither on nor off; a value outside a point's range; an unknown update command; a block with a bad checksum or byte count |
| 04 | Server device failure | An update command or block the present update state does not allow; `update_error` says why |
| 06 | Server device busy | A write to a process point while the device is in update mode |
| 0B | Gateway target device failed to respond | A unit identifier that is neither the device's nor `0xFF` |

Not carried: functions 15, 22, 23, and every other function; serial framing; more than one reply per identification request. A frame of function 15, 21, 22, or 23 is refused with exception 01 and still counted as a write, so a test that asserts "only reads were sent" catches it.

## The point map

`example_device::map::POINTS` is this table as data. Offsets count from zero. Numbers are raw; the scale turns a raw number into the value in its unit. "Volatile" points go back to their starting value on a restart.

| Point | Table | Offset | Data | Access | Scale, unit | Device range (raw) | Volatile | Start | Behaviour |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `inlet_valve` | coil | 4 | bit | read, write | | | yes | off | Commands the valve; the valve follows at the next tick |
| `fault_reset` | coil | 8 | bit | write | | | | | A command: on clears the fault latch. Reads as off |
| `pump_running` | discrete | 0 | bit | read | | | yes | off | On while the measured speed is above zero |
| `valve_open_feedback` | discrete | 1 | bit | read | | | yes | off | The valve's position sensor |
| `pump_speed_setpoint` | holding | 100 | u16 | read, write | 0.1 rpm | 0 to 14500 | yes | 0 | Outside the range: exception 03, nothing stored, latch bit 0 set |
| `dosing_rate` | holding | 101 | u16 | read, write | 0.01 l/min | 0 to 10000 | no | 250 | **Rounded** to the nearest multiple of 5: a read-back may differ |
| `heater_power_limit` | holding | 102 | u16 | read, write | 1 % | | no | 50 | **Clamped** to 80; the reply still echoes what was sent |
| `temperature_offset` | holding | 103 | i16 | read, write | 0.1 degC | -100 to 100 | no | 0 | Signed |
| `batch_target` | holding | 104, 105 | u32, low word first | read, write | 1 ml | 0 to 1000000 | no | 5000 | Both words in one frame, or exception 02 |
| `rated_speed` | holding | 106 | u16 | read | 0.1 rpm | | no | 14500 | Read only in a writable table: a write is exception 02 |
| `update_command` | holding | 900 | u16 | write | | 1 to 5 | | | See the update mechanism. Written alone. Reads as zero |
| `update_image_length` | holding | 901 | u16 | read, write | bytes | 1 to 4096 | yes | 0 | Declared before update mode is entered |
| `update_block` | holding | 1000 to 1122 | block | write | | | | | One block per frame, function 16 only. Reads as zeros |
| `fault_latch` | input | 7 | u16 | read | | | yes | 0 | **Reading it clears it.** Bit 0: a write was out of range. Bit 1: an image failed its check |
| `pump_speed_measured` | input | 30 | u16 | read | 0.1 rpm | | yes | 0 | Follows the setpoint by one ramp step per tick |
| `temperature` | input | 31 | i16 | read | 0.1 degC | | no | -35 | A fixed sensor value below zero, plus `temperature_offset` |
| `uptime_ticks` | input | 32, 33 | u32, high word first | read | | | yes | 0 | **Changes on its own:** one more at every tick |
| `firmware_major`, `firmware_minor`, `firmware_patch` | input | 100, 101, 102 | u16 | read | | | no | 2, 4, 0 | The running version |
| `serial_number` | input | 103, 104 | u32, high word first | read | | | no | 100123 | |
| `vendor` | input | 105 to 108 | text | read | | | no | `example` | Two characters per word, first in the high byte, padded with zero bytes |
| `model` | input | 109 to 117 | text | read | | | no | `example-controller` | |
| `firmware_checksum` | input | 118, 119 | u32, high word first | read | | | no | | The checksum of the running image |
| `active_slot` | input | 120 | u16 | read | | | no | 0 | 0 or 1 |
| `standby_valid` | input | 121 | u16 | read | | | no | 1 | 1 while a roll back has an image to start from |
| `standby_major`, `standby_minor`, `standby_patch` | input | 122, 123, 124 | u16 | read | | | no | 2, 3, 0 | The version in the other slot |
| `update_state` | input | 130 | u16 | read | | | yes | 0 | 0 idle, 1 receiving, 2 verified, 3 failed |
| `update_error` | input | 131 | u16 | read | | | yes | 0 | Why the last update step was refused; codes below |
| `update_next_block` | input | 132 | u16 | read | | | yes | 0 | The number the next block must carry |
| `update_bytes_received` | input | 133 | u16 | read | bytes | | yes | 0 | |
| `update_count` | input | 134 | u16 | read | | | no | 1 | Images ever committed. **It only rises**; a roll back does not lower it |

Every other address is refused with exception 02. Holding registers 100 to 106 and input registers 100 to 124 are contiguous, so each block can be read in one frame.

**The simulated outputs** are the pump's speed and the valve's position. They are what the device drives, as distinct from what it was told: the setpoint is held at once, and the pump reaches it one ramp step per tick; the valve command is held at once, and the valve follows at the next tick.

**Time.** The device has no clock. Its time moves in ticks: one tick after every frame it receives, and as many as a test asks for with `Control::advance`. `Control::set_process_frozen(true)` stops the ticks that frames cause, so a test can hold the process still. Nothing in the stand depends on the wall clock except the delay fault.

## The update mechanism

The device's own, reached through its registers. The protocol defines none. Two slots: the device runs from the active slot, and an update is received into the standby slot.

The declared sequence:

1. Write the image's length in bytes to `update_image_length`.
2. Write 1 (enter) to `update_command`. Refused unless the pump is stopped: setpoint and measured speed both zero. **Entering gives up the standby slot's image:** `standby_valid` becomes 0, and a roll back is refused until an update is committed.
3. Send the image in blocks. One block is one function 16 frame starting at holding register 1000: the block number from zero, the count of image bytes in the block (1 to 240), the CRC-16 of those bytes, then the bytes two to a word, first in the high byte, a last odd byte padded with zero. A block is taken once: the same number again is refused.
4. Write 2 (verify). The write is acknowledged; the result is in `update_state`: 2 (verified), or 3 (failed) with `update_error`.
5. Write 3 (commit), allowed only when verified. The device answers, makes the standby slot the active one, adds one to `update_count`, and restarts: every link closes and volatile points start again. Afterwards it reports the new version and `firmware_checksum`.
6. Or write 4 (abort) at any point in update mode: back to idle, the received bytes discarded, the standby slot still empty.

Write 5 (roll back), when idle, starts the device from the standby slot again and restarts it. It is refused when the standby slot holds no valid image.

In update mode, reads work and writes to process points are refused with exception 06.

| `update_error` | Meaning |
| --- | --- |
| 0 | Nothing was refused |
| 1 | The command or block is not allowed in the present update state |
| 2 | The pump is not stopped |
| 3 | No image length was declared |
| 4 | A block did not carry the next block number |
| 5 | A block's byte count is wrong, or it runs past the declared length |
| 6 | A block's checksum does not match |
| 7 | Fewer bytes were received than declared |
| 8 | The image does not have the form of an image |
| 9 | The image's checksum does not match |
| 10 | The image is for another model |
| 11 | The device refused a well-formed image (the `refuse_image` setting) |
| 12 | A roll back was refused: the standby slot holds no valid image |

**The image.** `firmware::build_image(model_code, version, payload)` makes one, as the device's imaginary maker would issue it. All numbers high byte first: the four bytes `EXFW`, the model code (`firmware::MODEL_CODE` for this device), the three numbers of the version, the payload length, the payload, and the CRC-32 of everything before it. The largest image is 4096 bytes. `firmware::image_checksum(image)` is the value the device reports in `firmware_checksum` once the image runs. The block checksum is CRC-16 with polynomial `0xA001` reflected and start value `0xFFFF`; the image checksum is the CRC-32 of Ethernet and zip files.

**What the device checks, and what it does not.** It checks the image's form, its checksum, and its model code. It checks no signature: an operation over this device declares that the device does not check the maker's signature.

**What cannot be undone.** `update_count` rises on every commit and never falls. Entering update mode destroys the standby slot's image.

**The recovery route.** A device that does not start after a commit is recovered by a person at the device, who starts it from the slot it ran before. In the stand that is `Control::recover()`. It returns `false`, and starts the device from the same slot, if the other slot held no valid image.

## The control interface

`TestDevice::control()` returns a `Control`: an in-process handle, cheap to clone. Nothing it does travels over the link, no fault applies to it, and looking never changes the device, not even a point that a read over the link would clear.

| Call | Reports or does |
| --- | --- |
| `reset()` | Restores the starting state and power; clears the frame log, the write counts, every fault, and every setting. Open links stay open |
| `inspect()` | A `Snapshot`: power, every point by name, the outputs, the firmware state, the number of frames, the write counts, the open links, the restarts, the reverts, the number of faults |
| `value(name)`, `number(name)`, `bit(name)` | One point's present value, by its name in the point map. Numbers are raw |
| `word(table, offset)` | The word, or the bit as 0 or 1, at an address; `None` where there is no point |
| `outputs()` | The pump's speed and the valve's position |
| `firmware()` | The running version and checksum, the active slot, both slots, the update state and error, the next block, the bytes received, the update count |
| `frames()` | Every frame received since the last reset, in order: its sequence number, link, transaction and unit identifiers, function, kind (`Read`, `Write`, `Other`), the decoded `Request`, the raw bytes, whether the device carried it out (`applied`), what it answered (`Normal`, `Exception(code)`, `None`), and the fault that applied |
| `clear_frames()` | Forgets the frames so far |
| `write_count(name)` | How many times a point was written. A refused write does not count |
| `set_fault(fault)`, `faults()`, `clear_faults()` | The frame faults below |
| `set_revert_on_link_loss(on)` | The device puts itself in its safe state when its last link closes |
| `set_stay_down_after_restart(on)` | The device does not come back from its next restart |
| `set_refuse_image(on)` | The device refuses a well-formed image at verify |
| `set_process_frozen(on)` | Frames no longer move the device's time |
| `set_firmware_version(version)` | The device reports another firmware version |
| `advance(ticks)` | Moves the device's time |
| `restart()` | Restarts the device: every link closes, volatile points start again |
| `power_off()`, `power_on()` | Takes the power away, and gives it back |
| `recover()` | The recovery route, above |
| `wait_until(within, condition)` | Waits until a condition on the snapshot holds, looking again after every change. A test waits here in place of sleeping |

## Faults

Every fault is a setting. A frame fault is a `Fault`: a kind, a scope (`All`, `Reads`, `Writes`, or one function code), a number of matching frames to let pass first (`after`), and a number of frames to apply to (`once`, `times`, or `always`). Each frame takes at most one fault: the first in the list that matches.

| Fault | What the device does | What it is for |
| --- | --- | --- |
| `DropLink { applied: true }` | Carries the request out, then closes the link with no reply | A write that may have landed: `possible_commit` |
| `DropLink { applied: false }` | Closes the link; nothing is carried out | A lost link before the frame took effect |
| `NoReply { applied }` | Sends nothing; the link stays open | A time-out, with or without the write stored |
| `Delay { ms }` | Carries the request out and answers after the wait | A slow reply |
| `Exception { code }` | Answers with that exception; nothing is carried out | A refusal on demand |
| `Fault::busy()` | Exception 06 | The device busy |
| `CorruptReply(TransactionId)` | Carries the request out; the reply has another transaction identifier | A reply that is not for the request |
| `CorruptReply(LastByte)` | Carries the request out; the reply's last byte is inverted | A wrong echo of a write; a wrong value in a read |
| `CorruptReply(Truncated)` | Carries the request out; the reply stops one byte short and the link closes | A reply cut off |
| `Restart` | Restarts in place of handling the frame | A restart in the middle of a run |

Settings and calls that are faults too: `restart()`, `power_off()`, `set_stay_down_after_restart`, `set_revert_on_link_loss`, `set_refuse_image`, and `set_firmware_version` (a firmware version the map was not proven against).

**A device that is down** accepts a link and closes it at once. No frame is read and none is recorded. The port stays the same, so the device can come back on it.

**Link loss.** With `set_revert_on_link_loss(true)`, the device sets the pump setpoint to zero and closes the valve when its last open link closes. It does so at once, with no settle time, because the stand has no clock. When a fault closes the link, the device has reverted before the client can see the link close. When the client closes the link, the device notices a moment later: a test waits with `wait_until(.., |now| now.open_links == 0)` before it opens another.

## Cases

Each case is a behaviour of the device that makes a device hard to operate. The stand's own tests, in `tests/cases.rs`, prove each with a small client written in the test.

| # | The device does this | Risk it exercises |
| --- | --- | --- |
| 1 | Holds bits, words, signed words, scaled values, text, and two-word values in both word orders | Encoding a register by its map |
| 2 | Reports what it is, in registers and by the protocol's identification function | Identify before acting; a firmware version outside the map |
| 3 | Refuses a value outside its own range, and latches the refusal | `rejected`, `device_refused`; the range checked twice |
| 4 | Clamps one point and rounds another, and still echoes what was sent | An acknowledgement is not a read-back; `mismatch` |
| 5 | Refuses a write to a read-only point, half a two-word point, and an unmapped address | Nothing outside the declared map |
| 6 | Clears a point when it is read | `read_effect`: a read is not always without effect |
| 7 | Has command points that read as nothing | `submit`: a write with nothing to read back |
| 8 | Changes on its own: an uptime counter, a pump that lags its setpoint, a valve that follows its command | `confirm_by`: the world is a second fact |
| 9 | Applies a write and drops the link before the reply | `possible_commit`; no automatic retry |
| 10 | Answers late, not at all, with an exception, busy, or with a damaged reply | `transient` on reads; `unexpected_reply` |
| 11 | Restarts, losing its volatile points and keeping the rest | Reconcile after a restart |
| 12 | Reverts to its safe state when the link is lost | `on_link_loss`; a reverted value is not a mismatch |
| 13 | Goes down and does not come back | `device_not_returned` |
| 14 | Takes a firmware image through its own sequence, checks it, commits it, and can roll back | The update operation, with one commit point |
| 15 | Refuses an image that is damaged, for another model, or simply refused; refuses update mode while the pump runs | The update's preconditions and stops |
| 16 | Destroys its standby image on entering update mode, and counts every commit | What cannot be undone; the recovery route |
| 17 | Answers for one unit identifier, and as a gateway would for any other | The unit address on a shared bus |

## Not imitated yet

- A console on a serial line or a raw socket, the other kind of link section 310 gives the pack.
- A register that holds a floating-point number.
- A signature on the image. The device checks a checksum and a model code only.
- A settle time before the device reverts on link loss. It reverts at once.
- An object dictionary in place of a register table.
