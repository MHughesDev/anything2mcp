name: pack-device

# The device pack, mode `device`

**Latches onto.** A device reached over a link, through its registers. Surface kind: `device_link`, with `address`, `protocol`, `unit`, and `serials`: the serial of the device in each environment, by environment name. Nothing is written where no serial is declared for the run's environment.

**File kind.** `registers/`: a device map. One at a time on one link.

**Verbs.** `read`, `update`, `submit`.

**Steps.** `open`, `build_frame`, `write_frame`, `read_frame`, `expect`.

## A device map, shortened, from the `example` target

```yaml
device_map: controller.main
surface: controller
model: example-controller
firmware: ["2.4.0", "2.4.1"]
identify:
  reads: [vendor, model, firmware_version, serial]
pace: {min_gap: 1ms, reply_within: 2s, max_rate: 500/s}
registers:
  heater_power_limit:
    address: {table: holding, offset: 102}
    data: u16
    type: integer
    access: read_write
    range: {min: 0, max: 100}
    hazard: none
  pump_speed_setpoint:
    address: {table: holding, offset: 100}
    data: u16
    scale: "0.1"
    type: pump_speed_rpm
    access: read_write
    range: {min: "0", max: "1450"}
    hazard: physical
    confirm_by: {register: pump_speed_measured, within: "25", settle: 5s}
safe_state:
  operation: example_pump_stop
  holds: {pump_speed_setpoint: "0", inlet_valve: "0"}
on_link_loss: {device_reverts_to_safe_state: true, after: 2s}
```

A binding of mode `device` names a `device_map`, `registers` (register to expression), and `read_back` (output to register):

```yaml
bindings:
  - mode: device
    verb: update
    device_map: controller.main
    registers: {heater_power_limit: input.limit}
    read_back: {limit: heater_power_limit}
```

## Rules

- The device is asked who it is first. A model, firmware, or serial that does not match is `unknown_variant`, `device_identity_mismatch`, for reads too.
- Every writable register declares a hazard. A writable number declares its safe `range`. A register that can only be written is a command, and its hazard is not `none`.
- A register that changes when read (`read_effect: clears`) is never a read-back.
- A frame is built and its digest recorded before it is sent. A write frame is marked as one that commits.
- A write that timed out is `possible_commit`. It is never retried. Reconcile reads the device.
- The setting `max_frames_per_run` caps the frames of a run, and `min_gap_ms` spaces them.
- Where the operation has no identity input, it is `irreversible`, and still read back.
- For anything physical, the map declares the safe state and the way to reach it, even though such writes are not built yet.

## Refuses

- A device with no declared serial, or whose identity does not match.
- A write outside a register's range.
- Discovery that does more than ask the device to identify itself (`discovery_identifies_only`).
- A write to a register with the hazard `physical`. It is not built: the step stops with `physical_not_built` and nothing is sent.
- Getting past a device's protection. Changing firmware other than through the device's own documented update.

## Codes

`input`: `value_out_of_range`, `schema_invalid`, `payload_unreadable`. `policy`: `surface_not_declared`, `device_serial_not_declared`, `discovery_identifies_only`, `frame_budget_exceeded`. `unknown_variant`: `device_identity_mismatch`, `physical_not_built`, `unexpected_reply`. `rejected`: `device_refused`. `transient`: `timeout`, `connection_lost`, `frame_corrupt`, `device_busy`. `possible_commit`: `no_confirmation`.
